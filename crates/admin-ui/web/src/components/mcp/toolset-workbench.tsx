import { Fragment, useMemo, useState } from 'react'
import { Add01Icon, ArrowDown01Icon, Layers01Icon, Search01Icon } from '@hugeicons/core-free-icons'
import { AppIcon } from '@/components/icons/app-icon'
import { McpServerIconMark } from '@/components/mcp/mcp-server-mark'
import { ToolsetConnectionDialog } from '@/components/mcp/toolset-connection-dialog'
import { IconTile } from '@/components/reui/icon-tile'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Checkbox } from '@/components/ui/checkbox'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldGroup,
  FieldLabel,
  FieldLegend,
  FieldSet,
} from '@/components/ui/field'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'
import { Separator } from '@/components/ui/separator'
import { Skeleton } from '@/components/ui/skeleton'
import { Spinner } from '@/components/ui/spinner'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'
import type {
  McpConnectionInfoPayload,
  McpServerView,
  McpToolsetView,
  McpToolView,
} from '@/types/api'

export interface ToolsetMembershipState {
  toolIds: string[]
  dirty: boolean
  loading: boolean
  error: string | null
  saving: boolean
}

export interface ToolsetWorkbenchProps {
  toolsets: McpToolsetView[]
  servers: McpServerView[]
  tools: McpToolView[]
  selectedId: string | null
  memberships: Record<string, ToolsetMembershipState>
  catalogPending: boolean
  catalogError: string | null
  busy?: boolean
  loadConnectionInfo: () => Promise<McpConnectionInfoPayload>
  onRetryCatalog: () => void
  onRetryMembership: (id: string) => void
  onSelect: (id: string) => void
  onEdit: (id: string) => void
  onSave: (id: string) => void
  onCreate: () => void
  onDisable: (id: string) => void
  onAccess: () => void
  onToggleTool: (id: string, checked: boolean) => void
  onRemoveUnavailable: (id: string) => void
}

export function ToolsetWorkbench(props: ToolsetWorkbenchProps) {
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState('all')
  const selected = props.toolsets.find((set) => set.id === props.selectedId)
  const term = query.trim().toLowerCase()
  const visibleSets = props.toolsets.filter(
    (set) =>
      (filter === 'all' || set.status === filter) &&
      `${set.display_name} ${set.toolset_key} ${set.description ?? ''}`
        .toLowerCase()
        .includes(term),
  )
  const selectableIds = useMemo(() => {
    const activeServers = new Set(
      props.servers.filter((server) => server.status === 'active').map((server) => server.id),
    )
    return new Set(
      props.tools
        .filter((tool) => tool.is_active && activeServers.has(tool.server_id))
        .map((tool) => tool.id),
    )
  }, [props.servers, props.tools])

  return (
    <section className="flex min-w-0 flex-col gap-6" aria-label="Tool Sets workbench">
      <header className="flex flex-wrap items-start justify-between gap-4">
        <div className="flex min-w-0 flex-col gap-2">
          <div className="flex items-center gap-2">
            <h1 className="text-2xl font-semibold tracking-tight">Tool Sets</h1>
            <Badge variant="secondary">{props.toolsets.length}</Badge>
          </div>
          <p className="text-muted-foreground text-sm">
            Bring the right tools together for each team and workflow.
          </p>
        </div>
        <Button type="button" onClick={props.onCreate} disabled={props.busy}>
          <AppIcon icon={Add01Icon} data-icon="inline-start" aria-hidden />
          New tool set
        </Button>
      </header>
      <Card className="min-w-0">
        <CardHeader>
          <CardTitle>Tool set workbench</CardTitle>
          <CardDescription>Choose a set and update its tool selection.</CardDescription>
        </CardHeader>
        <CardContent className="flex min-w-0 flex-col gap-6">
          <div className="flex min-w-0 flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
            <InputGroup className="w-full sm:max-w-xs">
              <InputGroupAddon>
                <AppIcon icon={Search01Icon} aria-hidden />
              </InputGroupAddon>
              <InputGroupInput
                aria-label="Search tool sets"
                placeholder="Search tool sets…"
                value={query}
                onChange={(event) => setQuery(event.target.value)}
              />
            </InputGroup>
            <ToggleGroup
              type="single"
              value={filter}
              onValueChange={(value) => {
                if (value) setFilter(value)
              }}
              spacing={1}
              variant="outline"
              aria-label="Tool set status"
            >
              <ToggleGroupItem value="all">All sets</ToggleGroupItem>
              <ToggleGroupItem value="active">Active</ToggleGroupItem>
              <ToggleGroupItem value="disabled">Disabled</ToggleGroupItem>
            </ToggleGroup>
          </div>
          <div
            className="grid min-w-0 items-start gap-7 lg:grid-cols-[20rem_minmax(0,1fr)]"
            data-testid="toolset-workbench-layout"
          >
            <ToolsetNavigator {...props} visibleSets={visibleSets} selectableIds={selectableIds} />
            {selected ? (
              <ToolsetWorkspace {...props} selected={selected} selectableIds={selectableIds} />
            ) : (
              <Empty className="min-w-0 border">
                <EmptyHeader>
                  <EmptyTitle>Choose a tool set</EmptyTitle>
                  <EmptyDescription>
                    Select a set to choose its tools, or create a new one.
                  </EmptyDescription>
                </EmptyHeader>
              </Empty>
            )}
          </div>
        </CardContent>
      </Card>
    </section>
  )
}

function ToolsetWorkspace(
  props: ToolsetWorkbenchProps & { selected: McpToolsetView; selectableIds: Set<string> },
) {
  const { selected } = props
  return (
    <section
      className="flex max-w-full min-w-0 flex-col gap-6 overflow-hidden"
      aria-label="Tool set workspace"
      data-testid="mcp-toolset-detail"
    >
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div className="flex min-w-0 grow basis-64 flex-col gap-2">
          <h2 className="text-lg font-semibold wrap-anywhere">{selected.display_name}</h2>
          <p className="text-muted-foreground max-w-3xl text-sm wrap-anywhere">
            {selected.description || 'Choose the tools this set makes available.'}
          </p>
        </div>
        <div className="flex max-w-full flex-wrap gap-2">
          <ToolsetConnectionDialog
            toolset={selected}
            loadConnectionInfo={props.loadConnectionInfo}
          />
          <Button type="button" variant="ghost" size="sm" onClick={props.onAccess}>
            Manage access
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            aria-label={`Disable ${selected.display_name}`}
            disabled={props.busy || selected.status !== 'active'}
            onClick={() => props.onDisable(selected.id)}
          >
            Disable set
          </Button>
        </div>
      </div>
      <WorkbenchCatalog {...props} />
    </section>
  )
}

function ToolsetNavigator(
  props: ToolsetWorkbenchProps & { visibleSets: McpToolsetView[]; selectableIds: Set<string> },
) {
  const selected = props.toolsets.find((set) => set.id === props.selectedId)
  const pinned =
    selected && !props.visibleSets.some((set) => set.id === selected.id) ? selected : null
  return (
    <aside
      className="flex min-w-0 flex-col gap-4 border-b pb-6 lg:sticky lg:top-6 lg:border-r lg:border-b-0 lg:pr-6 lg:pb-0"
      aria-label="Tool set navigator"
    >
      <div className="flex items-center justify-between gap-3 text-sm">
        <h2 className="font-medium">Your tool sets</h2>
        <span className="text-muted-foreground text-xs" role="status">
          {props.visibleSets.length} of {props.toolsets.length}
        </span>
      </div>
      {props.visibleSets.length > 0 || pinned ? (
        <ToggleGroup
          type="single"
          orientation="vertical"
          spacing={1}
          value={props.selectedId ?? ''}
          onValueChange={(value) => {
            if (value) props.onSelect(value)
          }}
          className="w-full min-w-0 gap-1"
          aria-label="Choose a tool set"
        >
          {pinned ? (
            <div className="flex w-full min-w-0 flex-col gap-2">
              <p className="text-muted-foreground text-xs">Current set · outside this filter</p>
              <NavigatorItem {...props} set={pinned} />
            </div>
          ) : null}
          {props.visibleSets.map((set, index) => (
            <Fragment key={set.id}>
              {index > 0 || pinned ? <Separator /> : null}
              <NavigatorItem {...props} set={set} />
            </Fragment>
          ))}
        </ToggleGroup>
      ) : null}
      {props.visibleSets.length === 0 ? (
        <Empty>
          <EmptyHeader>
            <EmptyTitle>No matching tool sets</EmptyTitle>
            <EmptyDescription>Try another search, or create a tool set.</EmptyDescription>
          </EmptyHeader>
          <Button type="button" variant="outline" onClick={props.onCreate} disabled={props.busy}>
            New tool set
          </Button>
        </Empty>
      ) : null}
    </aside>
  )
}

function NavigatorItem(
  props: ToolsetWorkbenchProps & { set: McpToolsetView; selectableIds: Set<string> },
) {
  const { set } = props
  const state = props.memberships[set.id]
  const loading = !state || state.loading
  const known = state && !state.loading && !state.error
  const unavailable = state?.toolIds.some((id) => !props.selectableIds.has(id))
  const saving = state?.saving ?? false
  const canSave =
    known &&
    state.dirty &&
    !saving &&
    !props.busy &&
    !props.catalogPending &&
    !props.catalogError &&
    !unavailable
  return (
    <div
      className={cn(
        'hover:bg-muted flex w-full min-w-0 flex-col gap-2 rounded-lg border p-3 transition-colors',
        props.selectedId === set.id ? 'border-ring/60 bg-muted' : 'border-transparent',
      )}
      data-testid={`toolset-row-${set.id}`}
    >
      <div className="flex min-w-0 items-start gap-2">
        <ToggleGroupItem
          value={set.id}
          aria-label={`Select ${set.display_name}`}
          className="h-auto min-w-0 flex-1 justify-start gap-3 px-1 py-1 text-left whitespace-normal hover:bg-transparent data-[state=on]:bg-transparent"
        >
          <IconTile size="sm">
            <AppIcon icon={Layers01Icon} aria-hidden />
          </IconTile>
          <span className="min-w-0 flex-1 wrap-anywhere">{set.display_name}</span>
        </ToggleGroupItem>
        <Badge variant={set.status === 'active' ? 'success' : 'secondary'}>
          {set.status === 'active' ? 'Active' : 'Disabled'}
        </Badge>
      </div>
      <div className="flex flex-wrap items-center justify-between gap-2 pl-1">
        <span className="text-muted-foreground text-xs tabular-nums" role="status">
          {loading
            ? 'Loading tools…'
            : state.error
              ? 'Count unavailable'
              : `${state.toolIds.length} ${state.toolIds.length === 1 ? 'tool' : 'tools'}`}
          {known && state.dirty ? <span className="text-foreground"> · Unsaved</span> : null}
        </span>
        <div className="flex gap-1">
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="enabled:hover:border-ring"
            aria-label={`Edit ${set.display_name}`}
            onClick={() => props.onEdit(set.id)}
            disabled={props.busy || saving}
          >
            Edit
          </Button>
          <ToolsetSaveButton
            name={set.display_name}
            dirty={Boolean(known && state.dirty)}
            saving={saving}
            disabled={!canSave}
            onSave={() => props.onSave(set.id)}
          />
        </div>
      </div>
      {state?.error ? (
        <div className="flex flex-col items-start gap-2">
          <p className="text-destructive text-xs">Could not load saved tools.</p>
          <Button
            type="button"
            variant="outline"
            size="sm"
            aria-label={`Retry tools for ${set.display_name}`}
            onClick={() => props.onRetryMembership(set.id)}
          >
            Retry
          </Button>
        </div>
      ) : null}
    </div>
  )
}

function ToolsetSaveButton({
  name,
  dirty,
  saving,
  disabled,
  onSave,
}: {
  name: string
  dirty: boolean
  saving: boolean
  disabled: boolean
  onSave: () => void
}) {
  const button = (
    <Button
      type="button"
      variant={dirty ? 'default' : 'outline'}
      size="sm"
      aria-label={`Save ${name}`}
      onClick={onSave}
      disabled={disabled}
    >
      {saving ? <Spinner data-icon="inline-start" /> : null}
      {saving ? 'Saving…' : 'Save'}
    </Button>
  )
  if (!disabled) return button
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          tabIndex={0}
          role="group"
          aria-label={`Save ${name} unavailable`}
          className="focus-visible:ring-ring/50 inline-flex rounded-md outline-none focus-visible:ring-3"
        >
          {button}
        </span>
      </TooltipTrigger>
      <TooltipContent side="top" sideOffset={6}>
        {saving ? 'Saving changes…' : 'Select or change tools to save changes.'}
      </TooltipContent>
    </Tooltip>
  )
}

function groupCatalogTools(servers: McpServerView[], tools: McpToolView[], term: string) {
  const groups = new Map<string, { server: McpServerView; tools: McpToolView[] }>()
  for (const server of servers) {
    if (server.status === 'active') groups.set(server.id, { server, tools: [] })
  }
  for (const tool of tools) {
    const group = groups.get(tool.server_id)
    if (!group) continue
    const text = `${tool.display_name} ${tool.upstream_name} ${tool.description ?? ''} ${group.server.display_name}`
    if (text.toLowerCase().includes(term)) group.tools.push(tool)
  }
  return [...groups.values()].filter((group) => group.tools.length > 0)
}

function WorkbenchCatalog(
  props: ToolsetWorkbenchProps & { selected: McpToolsetView; selectableIds: Set<string> },
) {
  const [query, setQuery] = useState('')
  const term = query.trim().toLowerCase()
  const member = props.memberships[props.selected.id]
  const disabled = props.busy || !member || member.loading || Boolean(member.error) || member.saving
  const groups = useMemo(
    () => groupCatalogTools(props.servers, props.tools, term),
    [props.servers, props.tools, term],
  )
  const selectedToolIds = new Set(member?.toolIds)
  return (
    <div className="flex max-w-full min-w-0 flex-col gap-4" aria-label="Tool catalog">
      <div className="flex flex-col gap-1">
        <h3 className="font-medium">Available tools</h3>
        <p className="text-muted-foreground text-sm">
          Choose tools across your servers. Save changes from the navigator.
        </p>
      </div>
      <InputGroup>
        <InputGroupAddon>
          <AppIcon icon={Search01Icon} aria-hidden />
        </InputGroupAddon>
        <InputGroupInput
          aria-label="Search available tools"
          placeholder="Search tools or servers…"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          disabled={props.catalogPending || Boolean(props.catalogError)}
        />
      </InputGroup>
      {props.catalogPending ? (
        <div className="flex flex-col gap-3" role="status">
          <p className="text-muted-foreground text-sm">Loading tool catalog…</p>
          <Skeleton className="h-24 w-full" />
          <Skeleton className="h-24 w-full" />
        </div>
      ) : props.catalogError ? (
        <Alert variant="destructive">
          <AlertTitle>Catalog failed to load</AlertTitle>
          <AlertDescription>
            Your selections are kept. Load the catalog before saving.
            <Button type="button" variant="outline" onClick={props.onRetryCatalog}>
              Retry catalog
            </Button>
          </AlertDescription>
        </Alert>
      ) : (
        <>
          <UnavailableTools
            {...props}
            toolIds={member?.toolIds ?? []}
            disabled={Boolean(disabled)}
          />
          {member?.error ? (
            <Alert variant="destructive">
              <AlertTitle>Saved tools could not be loaded</AlertTitle>
              <AlertDescription>Retry from the navigator to edit this set.</AlertDescription>
            </Alert>
          ) : null}
          <div
            className="flex min-w-0 flex-col gap-5 lg:max-h-[calc(100dvh-23rem)] lg:min-h-72 lg:overflow-y-auto lg:pr-2"
            data-testid="toolset-catalog-groups"
          >
            {groups.length === 0 ? (
              <Empty>
                <EmptyHeader>
                  <EmptyTitle>{term ? 'No matching tools' : 'No tools available'}</EmptyTitle>
                  <EmptyDescription>
                    {term
                      ? 'Search by tool name or server.'
                      : 'Discover tools on an active MCP server to add them here.'}
                  </EmptyDescription>
                </EmptyHeader>
              </Empty>
            ) : (
              groups.map(({ server, tools }) => (
                <FieldSet key={server.id} className="min-w-0 gap-2">
                  <FieldLegend variant="label">
                    <span className="flex items-center gap-2">
                      <McpServerIconMark server={server} />
                      <span>{server.display_name}</span>
                      <span className="text-muted-foreground text-xs">
                        {tools.filter((tool) => tool.is_active).length} available
                      </span>
                    </span>
                  </FieldLegend>
                  <FieldGroup className="gap-0 overflow-hidden rounded-lg border">
                    {tools.map((tool) => (
                      <WorkbenchToolRow
                        key={tool.id}
                        tool={tool}
                        checked={selectedToolIds.has(tool.id)}
                        disabled={Boolean(disabled)}
                        onToggle={props.onToggleTool}
                      />
                    ))}
                  </FieldGroup>
                </FieldSet>
              ))
            )}
          </div>
        </>
      )}
    </div>
  )
}

function WorkbenchToolRow({
  tool,
  checked,
  disabled,
  onToggle,
}: {
  tool: McpToolView
  checked: boolean
  disabled: boolean
  onToggle: (id: string, checked: boolean) => void
}) {
  return (
    <Collapsible className="min-w-0 border-b last:border-b-0">
      <div className="flex items-start gap-2 p-3">
        <Field
          orientation="horizontal"
          className="min-w-0"
          data-disabled={disabled || !tool.is_active}
        >
          <Checkbox
            id={`workbench-tool-${tool.id}`}
            checked={checked}
            disabled={disabled || !tool.is_active}
            onCheckedChange={(value) => onToggle(tool.id, value === true)}
          />
          <FieldContent className="min-w-0">
            <FieldLabel htmlFor={`workbench-tool-${tool.id}`} className="wrap-anywhere">
              {tool.display_name}
            </FieldLabel>
            <FieldDescription className="line-clamp-2 wrap-anywhere">
              {tool.description}
            </FieldDescription>
            {!tool.is_active ? <Badge variant="secondary">Unavailable</Badge> : null}
          </FieldContent>
        </Field>
        <CollapsibleTrigger asChild>
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            aria-label={`Inspect ${tool.display_name}`}
          >
            <AppIcon icon={ArrowDown01Icon} aria-hidden />
          </Button>
        </CollapsibleTrigger>
      </div>
      <CollapsibleContent>
        <div className="bg-muted/30 flex min-w-0 flex-col gap-2 border-t p-3">
          <p className="text-muted-foreground font-mono text-xs wrap-anywhere">
            {tool.upstream_name}
          </p>
          <p className="text-xs font-medium">Input schema</p>
          <pre className="max-h-64 max-w-full overflow-x-auto rounded-md border p-3 text-xs">
            {JSON.stringify(tool.input_schema, null, 2)}
          </pre>
        </div>
      </CollapsibleContent>
    </Collapsible>
  )
}

function UnavailableTools(
  props: ToolsetWorkbenchProps & {
    toolIds: string[]
    selectableIds: Set<string>
    disabled: boolean
  },
) {
  const unavailable = props.toolIds.filter((id) => !props.selectableIds.has(id))
  if (unavailable.length === 0) return null
  return (
    <Alert>
      <AlertTitle>Some saved tools are unavailable</AlertTitle>
      <AlertDescription>
        These tools are inactive or belong to an unavailable server. Remove them before saving other
        tool changes.
        <ul className="flex min-w-0 flex-col gap-2">
          {unavailable.map((id) => {
            const tool = props.tools.find((item) => item.id === id)
            return (
              <li className="flex min-w-0 items-center justify-between gap-2" key={id}>
                <span className="min-w-0 text-sm wrap-anywhere">{tool?.display_name ?? id}</span>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  aria-label={`Remove ${tool?.display_name ?? id}`}
                  disabled={props.disabled}
                  onClick={() => props.onRemoveUnavailable(id)}
                >
                  Remove
                </Button>
              </li>
            )
          })}
        </ul>
      </AlertDescription>
    </Alert>
  )
}
