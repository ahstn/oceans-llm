import { useMemo, useState } from 'react'
import { useTable, type ColumnDef, type SortingState } from '@tanstack/react-table'
import { Add01Icon, ArrowRight01Icon, RefreshIcon } from '@hugeicons/core-free-icons'
import { AppIcon } from '@/components/icons/app-icon'
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
import type { McpServerView } from '@/types/api'
import { authLabel, discoveredAt, endpointHost, needsAttention, type CandidateProps } from './model'
import {
  CandidateToolbar,
  CatalogPrompt,
  DiscoveryBadge,
  NoServers,
  RegistrationBadge,
  ServerMark,
} from './shared'

export function RegistryCandidate(props: CandidateProps) {
  const active = props.allServers.filter((server) => server.status === 'active').length
  const attention = props.allServers.filter(needsAttention).length
  return (
    <section className="flex min-w-0 flex-col gap-6" aria-label="Registry design">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div className="flex flex-col gap-2">
          <div className="flex items-center gap-2">
            <h1 className="text-2xl font-semibold tracking-tight">MCP servers</h1>
            <Badge variant="secondary">{props.allServers.length}</Badge>
          </div>
          <p className="text-muted-foreground text-sm">
            Manage the connections that bring tools into your gateway.
          </p>
        </div>
        <div className="flex gap-2">
          <Button type="button" variant="outline" onClick={props.onCatalog}>
            Browse catalog
          </Button>
          <Button type="button" onClick={props.onAdd}>
            <AppIcon icon={Add01Icon} data-icon="inline-start" aria-hidden />
            Add server
          </Button>
        </div>
      </div>
      <div className="flex flex-wrap items-center gap-x-6 gap-y-2 text-sm">
        <span>
          <strong className="font-medium">{active}</strong>
          <span className="text-muted-foreground"> active registrations</span>
        </span>
        <span>
          <strong className="font-medium">
            {props.allServers.filter((server) => server.last_discovery_status === 'success').length}
          </strong>
          <span className="text-muted-foreground"> with successful discovery</span>
        </span>
        <Button
          type="button"
          variant={attention ? 'destructive' : 'ghost'}
          size="sm"
          onClick={() => props.onFilterChange('attention')}
        >
          {attention} {attention === 1 ? 'needs' : 'need'} attention
          <AppIcon icon={ArrowRight01Icon} aria-hidden data-icon="inline-end" />
        </Button>
      </div>
      <Card className="min-w-0">
        <CardHeader>
          <CardTitle>Server registry</CardTitle>
          <CardDescription>Registration and discovery are shown separately.</CardDescription>
        </CardHeader>
        <CardContent className="flex min-w-0 flex-col gap-5">
          <CandidateToolbar {...props} />
          {props.servers.length ? <RegistryTable {...props} /> : <NoServers onAdd={props.onAdd} />}
          <p className="text-muted-foreground text-xs" role="status">
            Showing {props.servers.length} of {props.allServers.length} servers
          </p>
        </CardContent>
      </Card>
      <CatalogPrompt onCatalog={props.onCatalog} />
    </section>
  )
}

function RegistryTable({ servers, onManage, onRefresh, refreshingId }: CandidateProps) {
  const [sorting, setSorting] = useState<SortingState>([])
  const columns = useMemo<ColumnDef<DataGridFeatures, McpServerView>[]>(
    () => [
      {
        accessorKey: 'display_name',
        header: ({ column }) => <DataGridColumnHeader column={column} title="Server" />,
        size: 240,
        cell: ({ row }) => (
          <button
            type="button"
            className="focus-visible:ring-ring flex min-w-0 items-center gap-3 rounded-sm text-left outline-none hover:underline focus-visible:ring-2"
            onClick={() => onManage(row.original)}
          >
            <ServerMark server={row.original} size="sm" />
            <span className="flex min-w-0 flex-col gap-1">
              <span className="font-medium">{row.original.display_name}</span>
              <span className="text-muted-foreground max-w-48 truncate text-xs">
                {endpointHost(row.original)}
              </span>
            </span>
          </button>
        ),
      },
      {
        accessorKey: 'auth_mode',
        header: 'Authentication',
        size: 140,
        cell: ({ row }) => (
          <span className="text-muted-foreground">{authLabel(row.original.auth_mode)}</span>
        ),
      },
      {
        accessorKey: 'last_discovery_status',
        header: 'Last discovery',
        size: 180,
        cell: ({ row }) => (
          <div className="flex flex-col items-start gap-1.5">
            <DiscoveryBadge server={row.original} />
            <span className="text-muted-foreground text-xs">{discoveredAt(row.original)}</span>
          </div>
        ),
      },
      {
        accessorKey: 'last_tool_count',
        header: ({ column }) => <DataGridColumnHeader column={column} title="Tools" />,
        size: 75,
        cell: ({ row }) => (
          <span className="font-mono tabular-nums">{row.original.last_tool_count ?? '—'}</span>
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
        size: 130,
        cell: ({ row }) => (
          <div className="flex items-center gap-2">
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={() => onManage(row.original)}
            >
              Manage
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              aria-label={`Refresh ${row.original.display_name}`}
              disabled={row.original.status !== 'active' || refreshingId !== null}
              onClick={() => onRefresh(row.original)}
            >
              <AppIcon icon={RefreshIcon} aria-hidden />
            </Button>
          </div>
        ),
      },
    ],
    [onManage, onRefresh, refreshingId],
  )
  const table = useTable({
    features: dataGridFeatures,
    columns,
    data: servers,
    getRowId: (server: McpServerView) => server.id,
    state: { sorting },
    onSortingChange: setSorting,
    initialState: { pagination: { pageIndex: 0, pageSize: 1000 } },
  })
  return (
    <>
      <div className="hidden min-w-0 md:block">
        <DataGrid
          table={table}
          recordCount={servers.length}
          tableLayout={{ rowBorder: true, headerBackground: true }}
        >
          <DataGridContainer>
            <div className="overflow-x-auto">
              <DataGridTable />
            </div>
          </DataGridContainer>
        </DataGrid>
      </div>
      <MobileRegistry servers={servers} onManage={onManage} />
    </>
  )
}

function MobileRegistry({ servers, onManage }: Pick<CandidateProps, 'servers' | 'onManage'>) {
  return (
    <div className="flex flex-col divide-y rounded-lg border md:hidden">
      {servers.map((server) => (
        <div className="flex min-w-0 flex-col gap-3 p-4" key={server.id}>
          <div className="flex items-center gap-3">
            <ServerMark server={server} size="sm" />
            <span className="min-w-0 flex-1 truncate text-sm font-medium">
              {server.display_name}
            </span>
            <RegistrationBadge server={server} />
          </div>
          <div className="flex flex-wrap items-center justify-between gap-2">
            <DiscoveryBadge server={server} />
            <Button type="button" size="sm" variant="outline" onClick={() => onManage(server)}>
              Manage
            </Button>
          </div>
        </div>
      ))}
    </div>
  )
}
