import type { FormEvent } from 'react'
import { toast } from 'sonner'

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
  McpServerView,
  McpToolView,
  RecommendedMcpServerView,
  UpdateMcpServerInput,
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

export function ServerDetail({
  server,
  tools,
  toolsPending,
  toolsError,
  refreshStatus,
  refreshErrorSummary,
  actionPending,
  onEdit,
  onDisable,
  onRefresh,
}: {
  server: McpServerView
  tools: McpToolView[]
  toolsPending: boolean
  toolsError: string | null
  refreshStatus: string | null
  refreshErrorSummary: string | null
  actionPending: boolean
  onEdit: (server: McpServerView) => void
  onDisable: (server: McpServerView) => void
  onRefresh: (server: McpServerView) => void
}) {
  return (
    <div className="flex min-w-0 flex-col gap-4" data-testid="mcp-server-detail">
      <div className="flex min-w-0 flex-col gap-3 rounded-md border p-4">
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

        <div className="grid gap-3 text-sm md:grid-cols-2 xl:grid-cols-4">
          <DetailMetric label="Server URL" value={server.server_url} mono />
          <DetailMetric label="Auth mode" value={server.auth_mode} />
          <DetailMetric label="Timeout" value={`${server.timeout_ms} ms`} />
          <DetailMetric label="Last discovery" value={server.last_discovery_at ?? 'never'} />
          <DetailMetric
            label="Last success"
            value={server.last_successful_discovery_at ?? 'never'}
          />
          <DetailMetric label="Discovered tools" value={String(server.last_tool_count ?? 0)} />
          <DetailMetric label="Created" value={server.created_at} />
          <DetailMetric label="Updated" value={server.updated_at} />
        </div>
      </div>

      <div className="min-w-0 rounded-md border">
        <div className="flex items-center justify-between gap-2 border-b p-4">
          <div>
            <h3 className="font-medium">Discovered tools</h3>
            <p className="text-sm text-[var(--color-text-muted)]">
              Active state, schema hash, schema version, and discovery timestamps.
            </p>
          </div>
          <Badge variant="secondary">{tools.length}</Badge>
        </div>
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
            <Table className="min-w-[64rem]">
              <TableHeader>
                <TableRow>
                  <TableHead>Tool</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead>Schema</TableHead>
                  <TableHead>Version</TableHead>
                  <TableHead>First seen</TableHead>
                  <TableHead>Last seen</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {tools.map((tool) => (
                  <TableRow key={tool.id}>
                    <TableCell>
                      <div className="flex min-w-0 flex-col gap-1">
                        <span className="truncate font-medium">{tool.display_name}</span>
                        <span className="truncate font-mono text-xs text-[var(--color-text-muted)]">
                          {tool.upstream_name}
                        </span>
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
            {isCreate ? 'Register a Streamable HTTP MCP endpoint.' : 'Update endpoint and auth settings.'}
          </DialogDescription>
        </DialogHeader>
        <form className="flex flex-col gap-4" onSubmit={onSubmit}>
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
      <div className="uppercase tracking-wide">{label}</div>
      <div className="truncate font-medium text-[var(--color-text)]">{value}</div>
    </div>
  )
}

function DiscoveryStatusBadge({ status }: { status?: string | null }) {
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
      <div className="text-xs uppercase text-[var(--color-text-muted)]">{label}</div>
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
