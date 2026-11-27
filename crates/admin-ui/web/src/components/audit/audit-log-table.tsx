import { useMemo, useState } from 'react'
import { useTable, type ColumnDef } from '@tanstack/react-table'

import { INNER_PANEL } from '@/components/shared/inner-panel'
import { SearchField } from '@/components/shared/search-field'
import {
  DataGrid,
  DataGridContainer,
  dataGridFeatures,
  type DataGridFeatures,
} from '@/components/reui/data-grid/data-grid'
import { DataGridTable } from '@/components/reui/data-grid/data-grid-table'
import {
  Frame,
  FrameDescription,
  FrameFooter,
  FrameHeader,
  FramePanel,
  FrameTitle,
} from '@/components/reui/frame'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { EMPTY_VALUE } from '@/lib/format'
import { cn } from '@/lib/utils'
import type { AuditLogFiltersInput, AuditLogPageView, AuditLogSummaryView } from '@/types/api'

import { AuditEntrySheet } from './audit-entry-sheet'
import {
  actionVariant,
  actionVerbLabel,
  actorKindLabel,
  actorLabel,
  formatAuditTimestamp,
  targetTypeLabel,
} from './audit-meta'

export type AuditLogTableProps = {
  page: AuditLogPageView
  search: AuditLogFiltersInput
  pending: boolean
  /** Replaces the server-side filters and returns to the newest entries. */
  onSearch: (filters: AuditLogFiltersInput) => void
}

/** Filterable, cursor-paged list of admin audit entries with a detail sheet. */
export function AuditLogTable({ page, search, pending, onSearch }: AuditLogTableProps) {
  const [selected, setSelected] = useState<AuditLogSummaryView | null>(null)
  const filters = withoutCursor(search)
  const update = (patch: AuditLogFiltersInput) => {
    const next: AuditLogFiltersInput = { ...filters, ...patch }
    for (const key of Object.keys(next) as (keyof AuditLogFiltersInput)[]) {
      if (next[key] === '' || next[key] === undefined) delete next[key]
    }
    onSearch(next)
  }
  const hasFilters = Object.keys(filters).some((key) => key !== 'limit')
  const selectActor = (actor_user_id: string) => {
    if (!pending) update({ actor_user_id })
  }

  return (
    <Frame spacing="default" className="min-w-0">
      <FrameHeader className="flex flex-col gap-3">
        <div className="flex flex-col gap-0.5">
          <FrameTitle>Control-plane changes</FrameTitle>
          <FrameDescription>
            Who changed what, newest first. Secrets, tokens and invite links are never recorded.
          </FrameDescription>
        </div>
        <div className="flex flex-wrap items-center gap-2" data-testid="audit-log-toolbar">
          <Select
            value={search.target_type || 'all'}
            disabled={pending}
            onValueChange={(value) => update({ target_type: value === 'all' ? '' : value })}
          >
            <SelectTrigger size="sm" aria-label="Target type" className="w-52">
              <SelectValue placeholder="All targets" />
            </SelectTrigger>
            <SelectContent>
              <SelectGroup>
                <SelectItem value="all">All targets</SelectItem>
                {page.target_types.map((targetType) => (
                  <SelectItem key={targetType} value={targetType}>
                    {targetTypeLabel(targetType)}
                  </SelectItem>
                ))}
              </SelectGroup>
            </SelectContent>
          </Select>
          <SearchField
            label="Action"
            value={search.action}
            disabled={pending}
            onSubmit={(action) => update({ action })}
          />
          <SearchField
            label="Target ID"
            value={search.target_id}
            disabled={pending}
            onSubmit={(target_id) => update({ target_id })}
          />
          <SearchField
            label="Actor user ID"
            value={search.actor_user_id}
            disabled={pending}
            onSubmit={(actor_user_id) => update({ actor_user_id })}
          />
          {hasFilters ? (
            <Button variant="ghost" size="sm" disabled={pending} onClick={() => onSearch({})}>
              Clear
            </Button>
          ) : null}
        </div>
      </FrameHeader>
      <FramePanel className={cn(INNER_PANEL, 'min-w-0 p-0')}>
        <AuditEntries
          items={page.items}
          canClear={hasFilters}
          pending={pending}
          onClear={() => onSearch({})}
          onSelect={setSelected}
          onActor={selectActor}
        />
      </FramePanel>
      <FrameFooter>
        <div className="text-muted-foreground flex items-center justify-between gap-2 text-xs">
          <span className="tabular-nums">
            {page.items.length} {page.items.length === 1 ? 'entry' : 'entries'}
            {search.cursor ? ' (older page)' : ''}
          </span>
          <div className="flex gap-2">
            {search.cursor ? (
              <Button
                variant="outline"
                size="sm"
                disabled={pending}
                onClick={() => onSearch(filters)}
              >
                Newest
              </Button>
            ) : null}
            {page.next_cursor ? (
              <Button
                variant="outline"
                size="sm"
                disabled={pending}
                onClick={() => onSearch({ ...filters, cursor: page.next_cursor ?? undefined })}
              >
                Older
              </Button>
            ) : null}
          </div>
        </div>
      </FrameFooter>
      <AuditEntrySheet entry={selected} onOpenChange={(open) => !open && setSelected(null)} />
    </Frame>
  )
}

/** The entries as cards below `md` and as a grid from `md` up, or an empty state. */
function AuditEntries({
  items,
  canClear,
  pending,
  onClear,
  onSelect,
  onActor,
}: {
  items: AuditLogSummaryView[]
  canClear: boolean
  pending: boolean
  onClear: () => void
  onSelect: (entry: AuditLogSummaryView) => void
  onActor: (actorUserId: string) => void
}) {
  if (items.length === 0) {
    return (
      <Empty>
        <EmptyHeader>
          <EmptyTitle>No audit entries</EmptyTitle>
          <EmptyDescription>No control-plane changes match the current filters.</EmptyDescription>
        </EmptyHeader>
        {canClear ? (
          <Button variant="outline" size="sm" disabled={pending} onClick={onClear}>
            Clear filters
          </Button>
        ) : null}
      </Empty>
    )
  }
  return (
    <>
      {/* Both layouts stay in the DOM, so the server markup never depends on the viewport. */}
      <AuditCards items={items} onSelect={onSelect} onActor={onActor} />
      <div className="hidden md:block" data-testid="audit-log-grid">
        <AuditGrid items={items} onSelect={onSelect} onActor={onActor} />
      </div>
    </>
  )
}

function withoutCursor(search: AuditLogFiltersInput): AuditLogFiltersInput {
  const { cursor: _cursor, ...filters } = search
  return filters
}

function AuditGrid({
  items,
  onSelect,
  onActor,
}: {
  items: AuditLogSummaryView[]
  onSelect: (entry: AuditLogSummaryView) => void
  onActor: (actorUserId: string) => void
}) {
  const columns = useMemo<ColumnDef<DataGridFeatures, AuditLogSummaryView>[]>(
    () => [
      {
        accessorKey: 'occurred_at',
        header: 'Time',
        size: 200,
        cell: ({ row }) => (
          <span className="text-muted-foreground tabular-nums">
            {formatAuditTimestamp(row.original.occurred_at)}
          </span>
        ),
      },
      {
        accessorKey: 'action',
        header: 'Change',
        size: 200,
        cell: ({ row }) => (
          <span className="flex min-w-0 items-center gap-2">
            <span className="truncate">{targetTypeLabel(row.original.target_type)}</span>
            <Badge variant={actionVariant(row.original.action)}>
              {actionVerbLabel(row.original.action)}
            </Badge>
          </span>
        ),
      },
      {
        id: 'target',
        header: 'Target',
        size: 240,
        cell: ({ row }) => (
          <span className="flex min-w-0 flex-col">
            <span className="truncate">{row.original.target_label ?? '—'}</span>
            <span className="text-muted-foreground truncate font-mono text-xs">
              {row.original.target_id ?? ''}
            </span>
          </span>
        ),
      },
      {
        id: 'actor',
        header: 'Actor',
        size: 220,
        cell: ({ row }) => <ActorCell entry={row.original} onActor={onActor} />,
      },
      {
        accessorKey: 'request_ip',
        header: 'IP address',
        size: 140,
        cell: ({ row }) => (
          <span className="font-mono text-xs">{row.original.request_ip ?? '—'}</span>
        ),
      },
    ],
    [onActor],
  )
  const table = useTable({
    features: dataGridFeatures,
    columns,
    data: items,
    getRowId: (entry: AuditLogSummaryView) => entry.id,
    // Rows are one server page in server order; sorting here would only reorder that page.
    enableSorting: false,
    manualPagination: true,
  })

  return (
    <DataGrid
      table={table}
      recordCount={items.length}
      onRowClick={onSelect}
      tableLayout={{ rowBorder: true, headerBackground: true }}
    >
      <DataGridContainer>
        <div className="overflow-x-auto">
          <DataGridTable />
        </div>
      </DataGridContainer>
    </DataGrid>
  )
}

/** Below `md`, one card per entry. A foot button opens the entry, so the actor link stays usable. */
function AuditCards({
  items,
  onSelect,
  onActor,
}: {
  items: AuditLogSummaryView[]
  onSelect: (entry: AuditLogSummaryView) => void
  onActor: (actorUserId: string) => void
}) {
  return (
    <ul className="grid grid-cols-1 md:hidden" data-testid="audit-log-cards">
      {items.map((entry) => (
        <li key={entry.id} className="flex min-w-0 flex-col gap-3 border-b p-4 last:border-b-0">
          <div className="flex min-w-0 flex-wrap items-start justify-between gap-x-3 gap-y-1">
            <span className="flex min-w-0 flex-wrap items-center gap-2">
              <span className="truncate font-medium">{targetTypeLabel(entry.target_type)}</span>
              <Badge variant={actionVariant(entry.action)}>{actionVerbLabel(entry.action)}</Badge>
            </span>
            <span className="text-muted-foreground shrink-0 text-xs whitespace-nowrap tabular-nums">
              {formatAuditTimestamp(entry.occurred_at)}
            </span>
          </div>
          <div className="grid min-w-0 grid-cols-2 gap-3 text-sm">
            <div className="flex min-w-0 flex-col">
              <span className="text-muted-foreground text-xs">Target</span>
              <span className="truncate">{entry.target_label ?? EMPTY_VALUE}</span>
              {entry.target_id ? (
                <span className="text-muted-foreground truncate font-mono text-xs">
                  {entry.target_id}
                </span>
              ) : null}
            </div>
            <div className="flex min-w-0 flex-col">
              <span className="text-muted-foreground text-xs">Actor</span>
              <ActorCell entry={entry} onActor={onActor} />
            </div>
          </div>
          <Button variant="outline" size="sm" className="w-full" onClick={() => onSelect(entry)}>
            View details
          </Button>
        </li>
      ))}
    </ul>
  )
}

function ActorCell({
  entry,
  onActor,
}: {
  entry: AuditLogSummaryView
  onActor: (actorUserId: string) => void
}) {
  const actorUserId = entry.actor.user_id
  return (
    <span className="flex min-w-0 flex-col">
      {actorUserId ? (
        <button
          type="button"
          title="Filter by this actor"
          className="truncate text-left hover:underline"
          onClick={(event) => {
            event.stopPropagation()
            onActor(actorUserId)
          }}
        >
          {actorLabel(entry)}
        </button>
      ) : (
        <span className="truncate">{actorLabel(entry)}</span>
      )}
      <span className="text-muted-foreground text-xs">{actorKindLabel(entry.actor.kind)}</span>
    </span>
  )
}
