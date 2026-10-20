import { useMemo, useState } from 'react'
import { useTable, type ColumnDef, type OnChangeFn, type SortingState } from '@tanstack/react-table'
import { Add01Icon, ArrowRight01Icon, RefreshIcon, Search01Icon } from '@hugeicons/core-free-icons'
import { AppIcon } from '@/components/icons/app-icon'
import { McpServerIconMark } from '@/components/mcp/mcp-server-mark'
import { discoveryLabel, endpointHost, needsAttention } from '@/components/mcp/server-discovery'
import {
  DataGrid,
  DataGridContainer,
  dataGridFeatures,
  type DataGridFeatures,
} from '@/components/reui/data-grid/data-grid'
import { DataGridTable } from '@/components/reui/data-grid/data-grid-table'
import { DataGridColumnHeader } from '@/components/reui/data-grid/data-grid-column-header'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyTitle,
} from '@/components/ui/empty'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Spinner } from '@/components/ui/spinner'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import type { McpServerView } from '@/types/api'
import { EMPTY_VALUE, formatTimestamp } from '@/lib/format'
import { matchesOwner, ownerLabel, ownerOptions, visibilityLabel } from './server-ownership'

type ServerFilter = 'all' | 'attention' | 'active' | 'disabled'

interface RegistryProps {
  servers: McpServerView[]
  actionPending: boolean
  refreshingServerIds: string[]
  onManage: (serverId: string) => void
  onRefresh: (server: McpServerView) => void
  /** Omitted for a read-only viewer, who cannot register servers. */
  onAdd?: () => void
  onCatalog?: () => void
}

type RegistryRowsProps = Omit<RegistryProps, 'refreshingServerIds'> & {
  refreshingServerIds: ReadonlySet<string>
}

export function ServerRegistry(props: RegistryProps) {
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState<ServerFilter>('all')
  const [owner, setOwner] = useState('all')
  const [sorting, setSorting] = useState<SortingState>([])
  const active = props.servers.filter((server) => server.status === 'active').length
  const attention = props.servers.filter(needsAttention).length
  const discovered = props.servers.filter(
    (server) => server.last_discovery_status === 'success',
  ).length
  const search = query.trim().toLowerCase()
  const servers = props.servers.filter((server) => {
    const text = `${server.display_name} ${server.server_key} ${server.server_url} ${server.description ?? ''}`
    return (
      text.toLowerCase().includes(search) &&
      matchesOwner(server, owner) &&
      (filter === 'all' ||
        (filter === 'attention' ? needsAttention(server) : server.status === filter))
    )
  })

  return (
    <section className="flex min-w-0 flex-col gap-6" aria-label="MCP server registry">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div className="flex flex-col gap-2">
          <div className="flex items-center gap-2">
            <h1 className="text-2xl font-semibold tracking-tight">MCP servers</h1>
            <Badge variant="secondary">{props.servers.length}</Badge>
          </div>
          <p className="text-muted-foreground text-sm">
            {props.onAdd
              ? 'Manage the connections that bring tools into your gateway.'
              : 'The MCP servers your access lets you connect to.'}
          </p>
        </div>
        {props.onAdd ? (
          <div className="flex gap-2">
            <Button type="button" variant="outline" onClick={props.onCatalog}>
              Browse catalog
            </Button>
            <Button type="button" onClick={props.onAdd}>
              <AppIcon icon={Add01Icon} data-icon="inline-start" aria-hidden />
              Add server
            </Button>
          </div>
        ) : null}
      </div>
      <div className="flex flex-wrap items-center gap-x-6 gap-y-2 text-sm">
        <span>
          <strong className="font-medium">{active}</strong>
          <span className="text-muted-foreground"> active registrations</span>
        </span>
        <span>
          <strong className="font-medium">{discovered}</strong>
          <span className="text-muted-foreground"> with successful discovery</span>
        </span>
        <Button
          type="button"
          variant={attention ? 'destructive' : 'ghost'}
          size="sm"
          onClick={() => setFilter('attention')}
        >
          {attention} {attention === 1 ? 'needs' : 'need'} attention
          <AppIcon icon={ArrowRight01Icon} aria-hidden data-icon="inline-end" />
        </Button>
      </div>
      <Card className="min-w-0">
        <CardHeader>
          <CardTitle>Server registry</CardTitle>
          <CardDescription>
            Review registrations and the results of their latest discovery.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex min-w-0 flex-col gap-5">
          <RegistryToolbar
            query={query}
            filter={filter}
            owner={owner}
            owners={ownerOptions(props.servers)}
            onQueryChange={setQuery}
            onFilterChange={setFilter}
            onOwnerChange={setOwner}
          />
          {servers.length ? (
            <RegistryTable
              {...props}
              servers={servers}
              sorting={sorting}
              onSortingChange={setSorting}
            />
          ) : (
            <RegistryEmpty
              hasServers={props.servers.length > 0}
              onCatalog={props.onCatalog}
              onClear={() => {
                setQuery('')
                setFilter('all')
                setOwner('all')
              }}
            />
          )}
          <p className="text-muted-foreground text-xs" role="status">
            Showing {servers.length} of {props.servers.length} servers
          </p>
        </CardContent>
      </Card>
    </section>
  )
}

function RegistryEmpty({
  hasServers,
  onCatalog,
  onClear,
}: {
  hasServers: boolean
  onCatalog?: () => void
  onClear: () => void
}) {
  return (
    <Empty>
      <EmptyHeader>
        <EmptyTitle>{hasServers ? 'No matching servers' : 'No MCP servers'}</EmptyTitle>
        <EmptyDescription>
          {hasServers
            ? 'Try another search or clear the filters.'
            : onCatalog
              ? 'Add a catalog server or configure your own endpoint.'
              : 'You cannot connect to any MCP server yet. Ask your team admins for access.'}
        </EmptyDescription>
      </EmptyHeader>
      <EmptyContent>
        {hasServers ? (
          <Button type="button" variant="outline" onClick={onClear}>
            Clear filters
          </Button>
        ) : onCatalog ? (
          <Button type="button" variant="outline" onClick={onCatalog}>
            Find a server
          </Button>
        ) : null}
      </EmptyContent>
    </Empty>
  )
}

function RegistryToolbar({
  query,
  filter,
  owner,
  owners,
  onQueryChange,
  onFilterChange,
  onOwnerChange,
}: {
  query: string
  filter: ServerFilter
  owner: string
  owners: { value: string; label: string }[]
  onQueryChange: (query: string) => void
  onFilterChange: (filter: ServerFilter) => void
  onOwnerChange: (owner: string) => void
}) {
  return (
    <div className="flex min-w-0 flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
      <div className="flex w-full min-w-0 flex-col gap-3 sm:w-auto sm:flex-row sm:items-center">
        <InputGroup className="w-full sm:max-w-xs">
          <InputGroupInput
            aria-label="Search servers"
            placeholder="Search servers…"
            value={query}
            onChange={(event) => onQueryChange(event.target.value)}
          />
          <InputGroupAddon>
            <AppIcon icon={Search01Icon} aria-hidden />
          </InputGroupAddon>
        </InputGroup>
        <Select value={owner} onValueChange={onOwnerChange}>
          <SelectTrigger aria-label="Filter by owner" className="w-full sm:w-44">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectGroup>
              <SelectItem value="all">All owners</SelectItem>
              {owners.map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectGroup>
          </SelectContent>
        </Select>
      </div>
      <ToggleGroup
        type="single"
        variant="outline"
        size="sm"
        spacing={1}
        value={filter}
        aria-label="Filter servers"
        onValueChange={(value) => {
          if (value) onFilterChange(value as ServerFilter)
        }}
        className="grid w-full grid-cols-2 justify-start sm:flex sm:w-fit sm:flex-wrap"
      >
        <ToggleGroupItem value="all">All servers</ToggleGroupItem>
        <ToggleGroupItem value="attention">Needs attention</ToggleGroupItem>
        <ToggleGroupItem value="active">Active</ToggleGroupItem>
        <ToggleGroupItem value="disabled">Disabled</ToggleGroupItem>
      </ToggleGroup>
    </div>
  )
}

function RegistryTable(
  props: RegistryProps & { sorting: SortingState; onSortingChange: OnChangeFn<SortingState> },
) {
  const {
    servers,
    onManage,
    onRefresh,
    refreshingServerIds,
    actionPending,
    sorting,
    onSortingChange,
  } = props
  const refreshingIds = useMemo(() => new Set(refreshingServerIds), [refreshingServerIds])
  const columns = useRegistryColumns({ onManage, onRefresh, refreshingIds, actionPending })
  const table = useTable({
    features: dataGridFeatures,
    columns,
    data: servers,
    getRowId: (server: McpServerView) => server.id,
    state: { sorting },
    onSortingChange,
    manualPagination: true,
  })
  return (
    <>
      <div className="hidden min-w-0 md:block" data-testid="mcp-server-list">
        <DataGrid
          table={table}
          recordCount={servers.length}
          tableLayout={{ rowBorder: true, headerBackground: true }}
        >
          <DataGridContainer>
            <div className="overflow-x-auto" data-testid="mcp-server-table-scroll">
              <DataGridTable />
            </div>
          </DataGridContainer>
        </DataGrid>
      </div>
      <MobileRegistry {...props} refreshingServerIds={refreshingIds} />
    </>
  )
}

/** Columns of the desktop registry table. */
function useRegistryColumns({
  onManage,
  onRefresh,
  refreshingIds,
  actionPending,
}: Pick<RegistryProps, 'onManage' | 'onRefresh' | 'actionPending'> & {
  refreshingIds: ReadonlySet<string>
}) {
  return useMemo<ColumnDef<DataGridFeatures, McpServerView>[]>(
    () => [
      {
        accessorKey: 'display_name',
        header: ({ column }) => <DataGridColumnHeader column={column} title="Server" />,
        size: 260,
        cell: ({ row }) => (
          <button
            type="button"
            aria-label={`Open ${row.original.display_name}`}
            className="focus-visible:ring-ring flex min-w-0 items-center gap-3 rounded-sm text-left outline-none hover:underline focus-visible:ring-2"
            onClick={() => onManage(row.original.id)}
          >
            <McpServerIconMark server={row.original} />
            <span className="flex min-w-0 flex-col gap-1">
              <span className="font-medium">{row.original.display_name}</span>
              <span
                className="text-muted-foreground max-w-52 truncate text-xs"
                title={row.original.server_url}
              >
                {endpointHost(row.original)}
              </span>
            </span>
          </button>
        ),
      },
      {
        id: 'owner',
        accessorFn: (server: McpServerView) => ownerLabel(server),
        header: ({ column }) => <DataGridColumnHeader column={column} title="Owner" />,
        size: 160,
        cell: ({ row }) => <OwnerCell server={row.original} />,
      },
      {
        accessorKey: 'auth_mode',
        header: 'Authentication',
        size: 155,
        cell: ({ row }) => (
          <span className="text-muted-foreground">{authLabel(row.original.auth_mode)}</span>
        ),
      },
      {
        accessorKey: 'last_discovery_status',
        header: 'Last discovery',
        size: 185,
        cell: ({ row }) => <DiscoveryResult server={row.original} />,
      },
      {
        accessorKey: 'last_tool_count',
        header: ({ column }) => <DataGridColumnHeader column={column} title="Tools" />,
        size: 80,
        cell: ({ row }) => (
          <span
            className="font-mono tabular-nums"
            title="Tool count from the last successful discovery"
          >
            {row.original.last_tool_count ?? '—'}
          </span>
        ),
      },
      {
        accessorKey: 'status',
        header: 'Registration',
        size: 110,
        cell: ({ row }) => <RegistrationBadge server={row.original} />,
      },
      {
        id: 'actions',
        header: 'Actions',
        size: 145,
        cell: ({ row }) => (
          <RegistryActions
            server={row.original}
            actionPending={actionPending}
            refreshing={refreshingIds.has(row.original.id)}
            onManage={onManage}
            onRefresh={onRefresh}
          />
        ),
      },
    ],
    [onManage, onRefresh, refreshingIds, actionPending],
  )
}

function MobileRegistry({
  servers,
  actionPending,
  refreshingServerIds,
  onManage,
  onRefresh,
}: RegistryRowsProps) {
  return (
    <div
      className="flex flex-col divide-y rounded-lg border md:hidden"
      data-testid="mcp-server-list-mobile"
    >
      {servers.map((server) => (
        <div className="flex min-w-0 flex-col gap-3 p-4" key={server.id}>
          <div className="flex items-center gap-3">
            <McpServerIconMark server={server} />
            <div className="flex min-w-0 flex-1 flex-col gap-1">
              <span className="truncate text-sm font-medium">{server.display_name}</span>
              <span className="text-muted-foreground truncate text-xs" title={server.server_url}>
                {endpointHost(server)}
              </span>
            </div>
            <RegistrationBadge server={server} />
          </div>
          <div className="flex flex-wrap items-start justify-between gap-3">
            <DiscoveryResult server={server} />
            <span className="text-muted-foreground text-xs">
              {server.last_tool_count ?? '—'} {server.last_tool_count === 1 ? 'tool' : 'tools'}
            </span>
          </div>
          <OwnerCell server={server} />
          <div className="flex flex-wrap items-center justify-between gap-2">
            <span className="text-muted-foreground text-xs">{authLabel(server.auth_mode)}</span>
            <RegistryActions
              server={server}
              actionPending={actionPending}
              refreshing={refreshingServerIds.has(server.id)}
              onManage={onManage}
              onRefresh={onRefresh}
            />
          </div>
        </div>
      ))}
    </div>
  )
}

function RegistryActions({
  server,
  refreshing,
  actionPending,
  onManage,
  onRefresh,
}: Pick<RegistryProps, 'onManage' | 'onRefresh' | 'actionPending'> & {
  server: McpServerView
  refreshing: boolean
}) {
  return (
    <div className="flex items-center gap-2">
      <Button
        type="button"
        size="sm"
        variant="outline"
        aria-label={`${server.can_manage ? 'Manage' : 'View'} ${server.display_name}`}
        onClick={() => onManage(server.id)}
      >
        {server.can_manage ? 'Manage' : 'View'}
      </Button>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            aria-label={`Refresh ${server.display_name}`}
            disabled={
              actionPending || refreshing || server.status !== 'active' || !server.can_manage
            }
            onClick={() => onRefresh(server)}
          >
            {refreshing ? (
              <Spinner aria-label={`Refreshing ${server.display_name}`} />
            ) : (
              <AppIcon icon={RefreshIcon} aria-hidden />
            )}
          </Button>
        </TooltipTrigger>
        <TooltipContent>Refresh {server.display_name} tools</TooltipContent>
      </Tooltip>
    </div>
  )
}

function VisibilityBadge({ visibility }: { visibility: string }) {
  return (
    <Badge variant={visibility === 'team' ? 'outline' : 'secondary'}>
      {visibilityLabel(visibility)}
    </Badge>
  )
}

function OwnerCell({ server }: { server: McpServerView }) {
  return (
    <div className="flex min-w-0 flex-col items-start gap-1.5">
      <span className="max-w-40 truncate text-sm" title={server.owner_contact ?? undefined}>
        {ownerLabel(server)}
      </span>
      <VisibilityBadge visibility={server.visibility} />
    </div>
  )
}

function RegistrationBadge({ server }: { server: McpServerView }) {
  return (
    <Badge variant={server.status === 'active' ? 'success' : 'secondary'}>
      {server.status === 'active' ? 'Active' : 'Disabled'}
    </Badge>
  )
}

function DiscoveryResult({ server }: { server: McpServerView }) {
  const label = discoveryLabel(server.last_discovery_status)
  return (
    <div className="flex flex-col items-start gap-1.5">
      <Badge
        variant={needsAttention(server) ? 'destructive' : 'outline'}
        title={server.last_error_summary ?? undefined}
      >
        {label}
      </Badge>
      {server.last_discovery_at ? (
        <time
          className="text-muted-foreground text-xs whitespace-nowrap"
          dateTime={server.last_discovery_at}
          title={server.last_discovery_at}
        >
          {formatDiscoveryTime(server.last_discovery_at)}
        </time>
      ) : (
        <span className="text-muted-foreground text-xs">No discovery yet</span>
      )}
    </div>
  )
}

function authLabel(mode: string) {
  const labels: Record<string, string> = {
    none: 'No authentication',
    gateway_bearer_token: 'Gateway bearer token',
    gateway_static_header: 'Gateway static credential',
    oauth_obo: 'OAuth on-behalf-of',
    user_passthrough: 'User passthrough',
  }
  return labels[mode] ?? mode.replaceAll('_', ' ')
}

function formatDiscoveryTime(value: string) {
  const formatted = formatTimestamp(value)
  return formatted === EMPTY_VALUE ? 'Unknown discovery time' : formatted
}
