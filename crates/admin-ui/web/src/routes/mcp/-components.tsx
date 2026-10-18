import { useState, type FormEvent, type ReactNode } from 'react'
import { ArrowDown01Icon, ArrowRight01Icon, Copy01Icon } from '@hugeicons/core-free-icons'
import { toast } from 'sonner'

import { AppIcon } from '@/components/icons/app-icon'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
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
import { Separator } from '@/components/ui/separator'
import { Skeleton } from '@/components/ui/skeleton'
import { Switch } from '@/components/ui/switch'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { Textarea } from '@/components/ui/textarea'
import { cn } from '@/lib/utils'
import {
  PLATFORM_OWNER_LABEL,
  ownerLabel,
  visibilityLabel,
} from '@/components/mcp/server-ownership'
import type {
  AdminTeamOption,
  McpCredentialBindingView,
  McpServerView,
  McpToolView,
} from '@/types/api'
import { OauthDiscoveryPanel } from './-oauth-discovery'
import { ToolPolicyEditor } from './-tool-policy'
import type { CredentialBindingFormState, ServerFormState, ServerVisibility } from './-server-forms'

/** What the caller may set on a server's ownership in the server form. */
export type ServerOwnershipOptions = {
  canPublish: boolean
  /** Platform-owned servers are always published to the organisation. */
  platformOwned: boolean
  /** Teams a platform admin can register a new server for. */
  teams?: AdminTeamOption[]
  /**
   * Only platform admins review tool changes, so only they can require
   * approval for them.
   */
  canRequireApproval: boolean
}

const AUTH_MODES = [
  { value: 'none', label: 'None' },
  { value: 'gateway_static_header', label: 'Gateway static credential' },
  { value: 'gateway_bearer_token', label: 'Gateway bearer token' },
  { value: 'user_passthrough', label: 'User passthrough' },
  { value: 'oauth_obo', label: 'OAuth on-behalf-of' },
] as const

const AUTH_CONFIG_HINTS: Record<string, string> = {
  gateway_static_header:
    'Send the secret as header_name, optionally wrapped by value_template (for example "Token {{api_key}}"), or as a query_param such as "api_key". secret_ref names an env/OCEANS_MCP_DISCOVERY_* variable.',
  gateway_bearer_token: 'secret_ref names an env/OCEANS_MCP_DISCOVERY_* variable.',
  oauth_obo:
    "Use provider_key for a configured provider, or discover the server's own authorization server above. scopes is optional when the provider has defaults.",
}

export function ServerOverviewPanel({
  server,
  refreshStatus,
  refreshErrorSummary,
  ownershipControl,
}: {
  server: McpServerView
  refreshStatus: string | null
  refreshErrorSummary: string | null
  ownershipControl?: ReactNode
}) {
  return (
    <div className="flex min-w-0 flex-col gap-6">
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

      <div className="flex min-w-0 flex-col gap-2">
        <div className="text-subtle-foreground text-xs font-medium tracking-wide uppercase">
          Endpoint
        </div>
        <div className="text-foreground min-w-0 truncate font-mono text-sm">
          {server.server_url}
        </div>
      </div>

      <Separator />

      <dl className="grid text-sm sm:grid-cols-2">
        <OverviewDetail label="Auth mode" value={formatOverviewValue(server.auth_mode)} />
        <OverviewDetail label="Timeout" value={`${server.timeout_ms} ms`} />
        <OverviewDetail label="Last discovery" value={server.last_discovery_at ?? 'never'} />
        <OverviewDetail
          label="Last success"
          value={server.last_successful_discovery_at ?? 'never'}
        />
        <OverviewDetail label="Discovered tools" value={String(server.last_tool_count ?? 0)} />
        <OverviewDetail label="Owner" value={ownerLabel(server)} />
        <OverviewDetail label="Visibility" value={visibilityLabel(server.visibility)} />
        <OverviewDetail label="Contact" value={server.owner_contact ?? 'none'} />
        <OverviewDetail label="Created" value={server.created_at} />
        <OverviewDetail label="Updated" value={server.updated_at} />
      </dl>

      {server.can_manage ? null : (
        <p className="text-subtle-foreground text-sm">
          This server is managed by {ownerLabel(server)}. Ask {server.owner_contact ?? 'its owners'}{' '}
          to change it or to grant its tools.
        </p>
      )}
      {ownershipControl}
    </div>
  )
}

/** A platform admin's control for moving a server to a team or to the platform. */
export function ServerOwnershipTransfer({
  server,
  teams,
  pending,
  onTransfer,
}: {
  server: McpServerView
  teams: AdminTeamOption[]
  pending: boolean
  onTransfer: (ownerTeamId: string | null) => void
}) {
  const current = server.owner_team_id ?? 'platform'
  const [owner, setOwner] = useState(current)
  return (
    <div className="flex min-w-0 flex-col gap-3 rounded-md border p-4">
      <div className="flex flex-col gap-1">
        <h3 className="text-foreground text-sm font-semibold">Ownership</h3>
        <p className="text-subtle-foreground text-sm">
          Move the server to a team, or take it over for the platform. Platform-owned servers are
          published to the organisation.
        </p>
      </div>
      <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
        <Select value={owner} onValueChange={setOwner}>
          <SelectTrigger aria-label="Owning team" className="w-full sm:w-56">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectGroup>
              <SelectItem value="platform">{PLATFORM_OWNER_LABEL}</SelectItem>
              {teams.map((team) => (
                <SelectItem key={team.id} value={team.id}>
                  {team.name}
                </SelectItem>
              ))}
            </SelectGroup>
          </SelectContent>
        </Select>
        <Button
          type="button"
          variant="outline"
          disabled={pending || owner === current}
          onClick={() => onTransfer(owner === 'platform' ? null : owner)}
        >
          Transfer ownership
        </Button>
      </div>
    </div>
  )
}

function OverviewDetail({ label, value }: { label: string; value: string }) {
  return (
    <div className="border-border min-w-0 border-t py-3 sm:odd:pr-6 sm:even:pl-6">
      <dt className="text-subtle-foreground text-xs font-medium tracking-wide uppercase">
        {label}
      </dt>
      <dd className="text-foreground mt-1 truncate text-sm font-medium">{value}</dd>
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
  onToolUpdated,
}: {
  tools: McpToolView[]
  toolsPending: boolean
  toolsError: string | null
  selectedToolIds: string[]
  onToggleTool: (toolId: string) => void
  onClearSelection: () => void
  /** Omitted when the caller cannot manage tool sets. */
  onAddToToolset?: () => void
  /** Omitted when the caller cannot edit tool policy. */
  onToolUpdated?: (tool: McpToolView) => void
}) {
  const selected = new Set(selectedToolIds)
  const [expandedToolIds, setExpandedToolIds] = useState<Set<string>>(() => new Set())

  function toggleExpanded(toolId: string) {
    setExpandedToolIds((current) => {
      const next = new Set(current)
      if (next.has(toolId)) {
        next.delete(toolId)
      } else {
        next.add(toolId)
      }
      return next
    })
  }

  return (
    <div
      className="max-w-full min-w-0 overflow-hidden rounded-md border"
      data-testid="mcp-server-tools"
    >
      <div className="flex items-center justify-between gap-2 border-b p-4">
        <div>
          <h3 className="font-medium">Discovered tools</h3>
          <p className="text-subtle-foreground text-sm">
            {onAddToToolset
              ? 'Select tools to bundle into a toolset — no UUID copy-paste required.'
              : 'The tools this server offers.'}
          </p>
        </div>
        <Badge variant="secondary">{tools.length}</Badge>
      </div>

      {onAddToToolset && selectedToolIds.length > 0 ? (
        <div className="bg-muted flex flex-wrap items-center justify-between gap-2 border-b px-4 py-3">
          <span className="text-sm font-medium">
            {selectedToolIds.length} tool{selectedToolIds.length === 1 ? '' : 's'} selected
          </span>
          <div className="flex gap-2">
            <Button type="button" variant="ghost" size="sm" onClick={onClearSelection}>
              Clear
            </Button>
            {onAddToToolset ? (
              <Button type="button" size="sm" onClick={onAddToToolset}>
                Add to toolset
              </Button>
            ) : null}
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
        <div className="flex flex-col">
          {tools.map((tool) => (
            <ToolDisclosureRow
              key={tool.id}
              tool={tool}
              selected={selected.has(tool.id)}
              expanded={expandedToolIds.has(tool.id)}
              onToggleExpanded={() => toggleExpanded(tool.id)}
              onToolUpdated={onToolUpdated}
              // Selecting only feeds a tool set, so it is offered only with that action.
              onToggleTool={
                onAddToToolset
                  ? () => {
                      if (tool.is_active) {
                        onToggleTool(tool.id)
                      }
                    }
                  : undefined
              }
            />
          ))}
        </div>
      )}
    </div>
  )
}

/** A tool's checkbox, or a spacer that keeps the row's columns when selection is off. */
function ToolSelectBox({
  tool,
  selected,
  onToggle,
}: {
  tool: McpToolView
  selected: boolean
  onToggle?: () => void
}) {
  if (!onToggle) return <span aria-hidden className="size-4" />
  return (
    <input
      type="checkbox"
      aria-label={`Select ${tool.display_name}`}
      className="accent-primary size-4 cursor-pointer disabled:cursor-not-allowed disabled:opacity-50"
      checked={selected}
      disabled={!tool.is_active}
      onChange={onToggle}
    />
  )
}

function ToolDisclosureRow({
  tool,
  selected,
  expanded,
  onToggleExpanded,
  onToggleTool,
  onToolUpdated,
}: {
  tool: McpToolView
  selected: boolean
  expanded: boolean
  onToggleExpanded: () => void
  onToggleTool?: () => void
  onToolUpdated?: (tool: McpToolView) => void
}) {
  const schema = formatToolSchema(tool.input_schema)
  const description = tool.description?.trim()

  return (
    <Collapsible className="max-w-full min-w-0" open={expanded} onOpenChange={onToggleExpanded}>
      <div
        className={cn(
          'hover:bg-muted/40 max-w-full min-w-0 overflow-hidden border-t transition-colors',
          selected && 'bg-muted',
        )}
      >
        <div className="grid min-w-0 grid-cols-[auto_minmax(0,1fr)_auto_auto] items-center gap-3 px-4 py-3">
          <ToolSelectBox tool={tool} selected={selected} onToggle={onToggleTool} />
          <div className="min-w-0">
            <div className="truncate font-medium">{tool.display_name}</div>
            {description ? (
              <div className="text-subtle-foreground truncate text-sm">{description}</div>
            ) : null}
          </div>
          <ToolStatusBadge tool={tool} />
          <CollapsibleTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              aria-label={`${expanded ? 'Hide' : 'Show'} ${tool.display_name} schema`}
            >
              <AppIcon
                icon={expanded ? ArrowDown01Icon : ArrowRight01Icon}
                size={14}
                stroke={1.5}
                aria-hidden
              />
            </Button>
          </CollapsibleTrigger>
        </div>
        <CollapsibleContent className="max-w-full min-w-0 overflow-hidden">
          <div className="bg-background max-w-full min-w-0 overflow-hidden border-t px-4 py-4">
            <dl className="grid gap-3 text-sm md:grid-cols-3">
              <div className="min-w-0">
                <dt className="text-subtle-foreground text-xs font-medium">Tool ID</dt>
                <dd className="mt-1 flex min-w-0 items-center gap-1">
                  <span className="truncate font-mono text-xs">{tool.id}</span>
                  <CopyButton value={tool.id} label={`Copy ${tool.display_name} ID`} />
                </dd>
              </div>
              <div className="min-w-0">
                <dt className="text-subtle-foreground text-xs font-medium">Upstream name</dt>
                <dd className="mt-1 truncate font-mono text-xs">{tool.upstream_name}</dd>
              </div>
              <div>
                <dt className="text-subtle-foreground text-xs font-medium">Version</dt>
                <dd className="mt-1">{tool.schema_version}</dd>
              </div>
              {tool.title ? (
                <div className="min-w-0 md:col-span-3">
                  <dt className="text-subtle-foreground text-xs font-medium">Title</dt>
                  <dd className="mt-1 truncate">{tool.title}</dd>
                </div>
              ) : null}
              {toolAnnotationBadges(tool.annotations).length > 0 ? (
                <div className="min-w-0 md:col-span-3">
                  <dt className="text-subtle-foreground text-xs font-medium">Upstream hints</dt>
                  <dd className="mt-1">
                    <ToolAnnotationBadges tool={tool} />
                  </dd>
                </div>
              ) : null}
            </dl>
            {onToolUpdated ? <ToolPolicyEditor tool={tool} onToolUpdated={onToolUpdated} /> : null}
            <div className="mt-4 max-w-full min-w-0">
              <div className="text-subtle-foreground mb-2 text-xs font-medium">JSON schema</div>
              {schema ? (
                <div
                  className="bg-muted max-w-full min-w-0 overflow-hidden rounded-md border"
                  data-testid="mcp-tool-schema-scroll"
                >
                  <pre
                    className="max-h-72 max-w-full overflow-x-auto overflow-y-auto p-3 text-xs leading-relaxed"
                    data-testid="mcp-tool-schema-code"
                  >
                    <code className="block min-w-max">{schema}</code>
                  </pre>
                </div>
              ) : (
                <div className="bg-muted text-subtle-foreground rounded-md border p-3 text-sm">
                  No JSON schema available.
                </div>
              )}
            </div>
            <ToolOutputSchema schema={tool.output_schema} />
          </div>
        </CollapsibleContent>
      </div>
    </Collapsible>
  )
}

const reviewBadges: Record<string, { label: string; variant: 'warning' | 'destructive' }> = {
  pending_change: { label: 'Change pending', variant: 'warning' },
  pending_review: { label: 'Pending review', variant: 'warning' },
  rejected: { label: 'Rejected', variant: 'destructive' },
}

/** Active state, or the review state when a change waits or was rejected. */
function ToolStatusBadge({ tool }: { tool: McpToolView }) {
  const review = tool.is_active ? reviewBadges[tool.approval_status] : undefined
  if (review) {
    return <Badge variant={review.variant}>{review.label}</Badge>
  }
  return (
    <Badge variant={tool.is_active ? 'default' : 'secondary'}>
      {tool.is_active ? 'Active' : 'Inactive'}
    </Badge>
  )
}

function ToolOutputSchema({ schema }: { schema: McpToolView['output_schema'] }) {
  const formatted = schema ? formatToolSchema(schema) : null
  if (!formatted) {
    return null
  }
  return (
    <div className="mt-4 max-w-full min-w-0">
      <div className="text-subtle-foreground mb-2 text-xs font-medium">Output schema</div>
      <div className="bg-muted max-w-full min-w-0 overflow-hidden rounded-md border">
        <pre
          className="max-h-72 max-w-full overflow-x-auto overflow-y-auto p-3 text-xs leading-relaxed"
          data-testid="mcp-tool-output-schema-code"
        >
          <code className="block min-w-max">{formatted}</code>
        </pre>
      </div>
    </div>
  )
}

// Badges show only hints the upstream declared. A destructive hint is
// meaningless on a read-only tool, so it is hidden there. Omitted destructive
// and open-world hints default to true, so an explicit false gets its own badge.
function toolAnnotationBadges(annotations: McpToolView['annotations']) {
  const badges: { label: string; variant: 'success' | 'destructive' | 'outline' }[] = []
  if (annotations.read_only_hint === true) {
    badges.push({ label: 'Read-only', variant: 'success' })
  } else if (annotations.destructive_hint === true) {
    badges.push({ label: 'Destructive', variant: 'destructive' })
  } else if (annotations.destructive_hint === false) {
    badges.push({ label: 'Non-destructive', variant: 'outline' })
  }
  if (annotations.idempotent_hint === true) {
    badges.push({ label: 'Idempotent', variant: 'outline' })
  }
  if (annotations.open_world_hint === true) {
    badges.push({ label: 'Open world', variant: 'outline' })
  } else if (annotations.open_world_hint === false) {
    badges.push({ label: 'Closed world', variant: 'outline' })
  }
  return badges
}

function ToolAnnotationBadges({ tool }: { tool: McpToolView }) {
  const badges = toolAnnotationBadges(tool.annotations)
  if (badges.length === 0) {
    return null
  }
  return (
    <span
      className="flex flex-wrap items-center gap-1"
      data-testid="mcp-tool-annotations"
      title="Hints declared by the upstream server"
    >
      {badges.map((badge) => (
        <Badge key={badge.label} variant={badge.variant}>
          {badge.label}
        </Badge>
      ))}
    </span>
  )
}

function formatToolSchema(schema: unknown) {
  if (schema == null) {
    return null
  }

  if (typeof schema === 'string') {
    return schema
  }

  try {
    return JSON.stringify(schema, null, 2)
  } catch {
    return 'Unable to format schema'
  }
}

function CopyButton({ value, label }: { value: string; label: string }) {
  async function handleCopy() {
    try {
      if (!navigator.clipboard?.writeText) {
        throw new Error('Clipboard API unavailable')
      }
      await navigator.clipboard.writeText(value)
      toast.success('Tool ID copied')
    } catch {
      toast.error('Clipboard access failed')
    }
  }

  return (
    <Button
      type="button"
      variant="ghost"
      size="icon-sm"
      aria-label={label}
      onClick={() => void handleCopy()}
    >
      <AppIcon icon={Copy01Icon} size={14} stroke={1.5} aria-hidden />
    </Button>
  )
}

// Credential form and binding rows share owner, material, storage, and revocation invariants.
// oxlint-disable-next-line eslint/max-lines-per-function
export function CredentialBindingsPanel({
  bindings,
  form,
  pending,
  error,
  teamScoped = false,
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
                      <span className="text-subtle-foreground truncate font-mono text-xs">
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
      <div className="text-subtle-foreground border-t px-4 py-3 text-sm">
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
  ownership,
  onOpenChange,
  onFormChange,
  onSubmit,
}: {
  mode: 'create' | 'edit'
  open: boolean
  pending: boolean
  form: ServerFormState
  ownership?: ServerOwnershipOptions
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
            ownership={ownership}
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
  ownership,
  onFormChange,
}: {
  mode: 'create' | 'edit'
  form: ServerFormState
  ownership?: ServerOwnershipOptions
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
      <div className="min-w-0 md:col-span-2">
        <OauthDiscoveryPanel
          serverUrl={form.server_url}
          onApply={(authConfig) =>
            onFormChange({
              ...form,
              auth_mode: 'oauth_obo',
              auth_config: JSON.stringify(authConfig, null, 2),
            })
          }
        />
      </div>
      <ServerAuthFields form={form} onFormChange={onFormChange} />
      {(ownership?.canRequireApproval ?? true) ? (
        <ApproveToolChangesField
          checked={form.approve_tool_changes}
          onCheckedChange={(approve_tool_changes) =>
            onFormChange({ ...form, approve_tool_changes })
          }
        />
      ) : null}
      {ownership ? (
        <ServerOwnershipFields
          mode={mode}
          form={form}
          ownership={ownership}
          onFormChange={onFormChange}
        />
      ) : null}
    </FieldGroup>
  )
}

function ServerAuthFields({
  form,
  onFormChange,
}: {
  form: ServerFormState
  onFormChange: (form: ServerFormState) => void
}) {
  return (
    <>
      <Field>
        <FieldLabel>Auth mode</FieldLabel>
        <Select
          value={form.auth_mode}
          onValueChange={(value) => onFormChange({ ...form, auth_mode: value })}
        >
          <SelectTrigger aria-label="Auth mode">
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
        {AUTH_CONFIG_HINTS[form.auth_mode] ? (
          <FieldDescription>{AUTH_CONFIG_HINTS[form.auth_mode]}</FieldDescription>
        ) : null}
      </Field>
    </>
  )
}

function OwningTeamField({
  form,
  teams,
  onFormChange,
}: {
  form: ServerFormState
  teams: AdminTeamOption[]
  onFormChange: (form: ServerFormState) => void
}) {
  return (
    <Field>
      <FieldLabel>Owning team</FieldLabel>
      <Select
        value={form.owner_team_id || 'platform'}
        onValueChange={(value) =>
          onFormChange({
            ...form,
            owner_team_id: value === 'platform' ? '' : value,
            // A team-owned server starts team-only; the platform's is published.
            visibility: value === 'platform' ? 'organisation' : 'team',
          })
        }
      >
        <SelectTrigger aria-label="Owning team">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectGroup>
            <SelectItem value="platform">{PLATFORM_OWNER_LABEL}</SelectItem>
            {teams.map((team) => (
              <SelectItem key={team.id} value={team.id}>
                {team.name}
              </SelectItem>
            ))}
          </SelectGroup>
        </SelectContent>
      </Select>
    </Field>
  )
}

function ServerOwnershipFields({
  mode,
  form,
  ownership,
  onFormChange,
}: {
  mode: 'create' | 'edit'
  form: ServerFormState
  ownership: ServerOwnershipOptions
  onFormChange: (form: ServerFormState) => void
}) {
  // Publishing needs a platform admin unless the platform lets teams publish.
  const organisationDisabled = !ownership.canPublish && form.visibility !== 'organisation'
  const chooseOwner = mode === 'create' && ownership.teams !== undefined
  // When a platform admin picks the owner, the platform rule follows that choice.
  const platformOwned = chooseOwner ? form.owner_team_id === '' : ownership.platformOwned
  return (
    <>
      {chooseOwner ? (
        <OwningTeamField form={form} teams={ownership.teams ?? []} onFormChange={onFormChange} />
      ) : null}
      <Field>
        <FieldLabel>Visibility</FieldLabel>
        <Select
          value={form.visibility}
          disabled={platformOwned}
          onValueChange={(value) =>
            onFormChange({ ...form, visibility: value as ServerVisibility })
          }
        >
          <SelectTrigger aria-label="Visibility">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectGroup>
              <SelectItem value="team">Team only</SelectItem>
              <SelectItem value="organisation" disabled={organisationDisabled}>
                Organisation
              </SelectItem>
            </SelectGroup>
          </SelectContent>
        </Select>
        <FieldDescription>
          {platformOwned
            ? 'Platform-owned servers are published to the organisation.'
            : organisationDisabled
              ? 'Only a platform admin can publish this server to the organisation.'
              : 'Team only servers can be granted to your team only.'}
        </FieldDescription>
      </Field>
      <Field>
        <FieldLabel htmlFor="mcp-owner-contact">Owner contact</FieldLabel>
        <Input
          id="mcp-owner-contact"
          value={form.owner_contact}
          maxLength={256}
          onChange={(event) => onFormChange({ ...form, owner_contact: event.target.value })}
          placeholder={mode === 'create' ? 'Defaults to your email' : 'Email or channel'}
        />
      </Field>
    </>
  )
}

function ApproveToolChangesField({
  checked,
  onCheckedChange,
}: {
  checked: boolean
  onCheckedChange: (checked: boolean) => void
}) {
  return (
    <Field orientation="horizontal" className="md:col-span-2">
      <Switch id="mcp-approve-tool-changes" checked={checked} onCheckedChange={onCheckedChange} />
      <div className="flex flex-col gap-1">
        <FieldLabel htmlFor="mcp-approve-tool-changes">
          Require approval for tool changes
        </FieldLabel>
        <FieldDescription>
          New tools and changed definitions wait in Reviews until an admin approves them. A
          description-only change keeps the approved version served meanwhile.
        </FieldDescription>
      </div>
    </Field>
  )
}

export function ServerStatusBadge({ status }: { status: string }) {
  return (
    <Badge variant={status === 'active' ? 'default' : 'secondary'}>
      {status === 'active' ? 'Active' : 'Disabled'}
    </Badge>
  )
}

export function DiscoveryStatusBadge({ status }: { status?: string | null }) {
  const label = status ?? 'not run'
  const variant = status === 'success' ? 'default' : status ? 'secondary' : 'outline'
  return <Badge variant={variant}>{label}</Badge>
}

function formatOverviewValue(value: string) {
  return value.replaceAll('_', ' ')
}
