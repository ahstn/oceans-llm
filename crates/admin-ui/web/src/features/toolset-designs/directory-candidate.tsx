import { useMemo, useState } from 'react'
import { useTable, type ColumnDef, type SortingState } from '@tanstack/react-table'
import { Add01Icon, ArrowRight01Icon } from '@hugeicons/core-free-icons'
import { AppIcon } from '@/components/icons/app-icon'
import {
  DataGrid,
  DataGridContainer,
  dataGridFeatures,
  type DataGridFeatures,
} from '@/components/reui/data-grid/data-grid'
import { DataGridColumnHeader } from '@/components/reui/data-grid/data-grid-column-header'
import { DataGridTable } from '@/components/reui/data-grid/data-grid-table'
import { Frame, FramePanel } from '@/components/reui/frame'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import type { McpToolsetView } from '@/types/api'
import type { ToolsetCandidateProps } from './model'
import {
  DraftSummary,
  NoToolsets,
  SelectedSetHeader,
  ToolCatalog,
  ToolsetMark,
  ToolsetStatus,
  ToolsetToolbar,
} from './shared'

function updatedDate(value: string) {
  return new Intl.DateTimeFormat('en-GB', {
    day: 'numeric',
    month: 'short',
    year: 'numeric',
    timeZone: 'UTC',
  }).format(new Date(value))
}

export function DirectoryCandidate(props: ToolsetCandidateProps) {
  const active = props.allSets.filter((set) => set.status === 'active').length
  return (
    <section className="flex min-w-0 flex-col gap-6" aria-label="Directory design">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div className="flex flex-col gap-2">
          <div className="flex items-center gap-2">
            <h1 className="text-2xl font-semibold tracking-tight">Tool Sets</h1>
            <Badge variant="secondary">{props.allSets.length}</Badge>
          </div>
          <p className="text-muted-foreground max-w-xl text-sm">
            Curate server tools into focused collections for your teams and applications.
          </p>
        </div>
        <Button type="button" onClick={props.onCreate}>
          <AppIcon icon={Add01Icon} data-icon="inline-start" aria-hidden />
          New tool set
        </Button>
      </div>

      <div className="flex flex-wrap items-center gap-x-6 gap-y-2 text-sm">
        <span>
          <strong className="font-medium">{active}</strong>
          <span className="text-muted-foreground"> active tool sets</span>
        </span>
        <span>
          <strong className="font-medium">{props.allSets.length - active}</strong>
          <span className="text-muted-foreground"> disabled</span>
        </span>
      </div>

      <Card className="min-w-0">
        <CardHeader>
          <CardTitle>Tool set directory</CardTitle>
          <CardDescription>
            Find a collection, update its details, or prepare a new tool selection.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex min-w-0 flex-col gap-5">
          <ToolsetToolbar {...props} />
          {props.sets.length ? (
            <DirectoryTable {...props} />
          ) : (
            <NoToolsets onCreate={props.onCreate} />
          )}
          <p className="text-muted-foreground text-xs" role="status">
            Showing {props.sets.length} of {props.allSets.length} tool sets
          </p>
        </CardContent>
      </Card>

      <div className="flex flex-wrap items-center justify-between gap-3 px-1">
        <p className="text-muted-foreground max-w-xl text-sm">
          Tool sets define which tools belong together. Access rules decide who can use them.
        </p>
        <Button type="button" variant="ghost" onClick={props.onAccess}>
          Manage access
          <AppIcon icon={ArrowRight01Icon} data-icon="inline-end" aria-hidden />
        </Button>
      </div>
      <DirectoryEditor {...props} />
    </section>
  )
}

function DirectoryTable({ sets, onSelect }: ToolsetCandidateProps) {
  const [sorting, setSorting] = useState<SortingState>([])
  const columns = useMemo<ColumnDef<DataGridFeatures, McpToolsetView>[]>(
    () => [
      {
        accessorKey: 'display_name',
        header: ({ column }) => <DataGridColumnHeader column={column} title="Tool set" />,
        size: 440,
        cell: ({ row }) => (
          <div className="flex min-w-0 items-start gap-3 py-1">
            <ToolsetMark />
            <div className="flex min-w-0 flex-col gap-1">
              <button
                type="button"
                className="focus-visible:ring-ring w-fit rounded-sm text-left font-medium outline-none hover:underline focus-visible:ring-2"
                onClick={() => onSelect(row.original)}
              >
                {row.original.display_name}
              </button>
              <span className="text-muted-foreground max-w-sm truncate text-xs">
                {row.original.description || 'No description added'}
              </span>
              <span className="text-muted-foreground font-mono text-xs">
                {row.original.toolset_key}
              </span>
            </div>
          </div>
        ),
      },
      {
        accessorKey: 'status',
        header: 'Status',
        size: 120,
        cell: ({ row }) => <ToolsetStatus set={row.original} />,
      },
      {
        accessorKey: 'updated_at',
        header: ({ column }) => <DataGridColumnHeader column={column} title="Updated" />,
        size: 150,
        cell: ({ row }) => (
          <time className="text-muted-foreground" dateTime={row.original.updated_at}>
            {updatedDate(row.original.updated_at)}
          </time>
        ),
      },
      {
        id: 'actions',
        header: 'Actions',
        size: 110,
        cell: ({ row }) => (
          <Button type="button" size="sm" variant="outline" onClick={() => onSelect(row.original)}>
            Manage
          </Button>
        ),
      },
    ],
    [onSelect],
  )
  const table = useTable({
    features: dataGridFeatures,
    columns,
    data: sets,
    getRowId: (set: McpToolsetView) => set.id,
    state: { sorting },
    onSortingChange: setSorting,
    initialState: { pagination: { pageIndex: 0, pageSize: 1000 } },
  })
  return (
    <>
      <div className="hidden min-w-0 md:block">
        <DataGrid
          table={table}
          recordCount={sets.length}
          tableLayout={{ rowBorder: true, headerBackground: true }}
        >
          <DataGridContainer>
            <div className="overflow-x-auto">
              <DataGridTable />
            </div>
          </DataGridContainer>
        </DataGrid>
      </div>
      <MobileDirectory sets={sets} onSelect={onSelect} />
    </>
  )
}

function MobileDirectory({ sets, onSelect }: Pick<ToolsetCandidateProps, 'sets' | 'onSelect'>) {
  return (
    <div className="flex min-w-0 flex-col divide-y rounded-lg border md:hidden">
      {sets.map((set) => (
        <div className="flex min-w-0 flex-col gap-3 p-4" key={set.id}>
          <div className="flex min-w-0 items-start gap-3">
            <ToolsetMark />
            <div className="flex min-w-0 flex-1 flex-col gap-1">
              <span className="text-sm font-medium">{set.display_name}</span>
              <span className="text-muted-foreground text-xs break-words">
                {set.description || 'No description added'}
              </span>
              <span className="text-muted-foreground truncate font-mono text-xs">
                {set.toolset_key}
              </span>
            </div>
          </div>
          <div className="flex flex-wrap items-center justify-between gap-2">
            <ToolsetStatus set={set} />
            <Button type="button" size="sm" variant="outline" onClick={() => onSelect(set)}>
              Manage
            </Button>
          </div>
        </div>
      ))}
    </div>
  )
}

function DirectoryEditor(props: ToolsetCandidateProps) {
  return (
    <Dialog open={props.selected !== null} onOpenChange={(open) => !open && props.onSelect(null)}>
      <DialogContent className="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-5xl">
        <DialogHeader className="pr-8">
          <DialogTitle>Manage tool set</DialogTitle>
          <DialogDescription>
            Update collection details and choose the tools for your next saved selection.
          </DialogDescription>
        </DialogHeader>
        {props.selected && (
          <div className="flex min-w-0 flex-col gap-5">
            <SelectedSetHeader {...props} />
            <div className="grid min-w-0 items-start gap-5 lg:grid-cols-[minmax(0,1fr)_18rem]">
              <ToolCatalog {...props} />
              <Frame className="min-w-0" spacing="sm">
                <FramePanel>
                  <DraftSummary {...props} />
                </FramePanel>
              </Frame>
            </div>
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}
