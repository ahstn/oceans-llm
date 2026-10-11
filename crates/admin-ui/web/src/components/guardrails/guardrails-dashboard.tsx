import { useMemo, useState } from 'react'
import { useTable, type ColumnDef, type SortingState } from '@tanstack/react-table'

import {
  DataGrid,
  DataGridContainer,
  dataGridFeatures,
  type DataGridFeatures,
} from '@/components/reui/data-grid/data-grid'
import { DataGridColumnHeader } from '@/components/reui/data-grid/data-grid-column-header'
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
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
import type { GuardrailDecisionView } from '@/types/api'

import { ActionLegend, ActionMeter } from './action-meter'
import {
  PHASES,
  actionLabel,
  actionVariant,
  countBy,
  evaluatorLabel,
  formatLatency,
  formatTimestamp,
  percentile,
  phaseLabel,
  policyScopes,
  redactionLabel,
  ruleLabel,
} from './decision-meta'
import { DecisionSheet } from './decision-sheet'
import { DecisionPager, DecisionToolbar, type DashboardProps } from './decision-toolbar'

/** Dashboard grid: KPIs and an action-by-phase breakdown above a sortable data grid. */
export function GuardrailsDashboard(props: DashboardProps) {
  const { decisions, policies, search, onSearch } = props
  const [selected, setSelected] = useState<GuardrailDecisionView | null>(null)
  const items = decisions.items
  const byAction = countBy(items, (item) => item.action)
  const latencies = items.map((item) => item.latency_micros)

  return (
    <div className="flex min-w-0 flex-col gap-6">
      <Frame spacing="default" className="grid grid-cols-1 sm:grid-cols-2 xl:grid-cols-4">
        <Kpi label="Decisions" value={decisions.total.toLocaleString()} hint="Matching filters" />
        <Kpi
          label="Denied"
          value={(byAction.get('deny') ?? 0).toLocaleString()}
          hint={share(byAction.get('deny'), items.length)}
          badge={byAction.get('deny') ? 'destructive' : undefined}
        />
        <Kpi
          label="Transformed"
          value={(byAction.get('transformed') ?? 0).toLocaleString()}
          hint={share(byAction.get('transformed'), items.length)}
        />
        <Kpi
          label="p95 latency"
          value={formatLatency(percentile(latencies, 95))}
          hint={`Median ${formatLatency(percentile(latencies, 50))}`}
        />
      </Frame>

      <div className="grid min-w-0 gap-6 lg:grid-cols-[minmax(0,1fr)_20rem]">
        <Frame spacing="default" className="min-w-0">
          <FrameHeader className="flex-row flex-wrap items-center justify-between gap-2">
            <div className="flex flex-col gap-0.5">
              <FrameTitle>Action mix by phase</FrameTitle>
              <FrameDescription>Select a phase to filter the grid.</FrameDescription>
            </div>
            <ActionLegend />
          </FrameHeader>
          <FramePanel className="flex flex-col gap-1">
            {PHASES.map(({ id, label }) => {
              const phaseItems = items.filter((item) => item.phase === id)
              const active = search.phase === id
              return (
                <button
                  key={id}
                  type="button"
                  aria-pressed={active}
                  onClick={() =>
                    onSearch({ ...search, phase: active ? undefined : id, page: undefined })
                  }
                  className="hover:bg-muted aria-pressed:bg-muted dark:hover:bg-muted/50 grid grid-cols-[9rem_minmax(0,1fr)_2.5rem] items-center gap-3 rounded-md px-2 py-1.5 text-left text-sm transition-colors"
                >
                  <span className="truncate">{label}</span>
                  <ActionMeter counts={countBy(phaseItems, (item) => item.action)} />
                  <span className="text-muted-foreground text-right text-xs tabular-nums">
                    {phaseItems.length}
                  </span>
                </button>
              )
            })}
          </FramePanel>
        </Frame>

        <Frame spacing="default" className="min-w-0">
          <FrameHeader>
            <FrameTitle>Effective policies</FrameTitle>
            <FrameDescription>Read-only; configuration is authoritative.</FrameDescription>
          </FrameHeader>
          <FramePanel className="flex flex-col gap-3">
            {policyScopes(policies).map((scope) => (
              <div key={scope.key} className="flex min-w-0 flex-col gap-1">
                <div className="flex items-center gap-2 text-sm">
                  <span className="truncate font-medium">{scope.name}</span>
                  <Badge variant={scope.policy.enabled ? 'success' : 'outline'}>
                    {scope.policy.enabled ? scope.policy.mode : 'Disabled'}
                  </Badge>
                </div>
                <p className="text-muted-foreground flex min-w-0 items-center gap-1.5 text-xs">
                  <span className="truncate">{scope.policy.packs.join(', ') || 'No packs'}</span>
                  <span
                    aria-hidden
                    className="bg-muted-foreground/40 size-1 shrink-0 rounded-full"
                  />
                  <span className="truncate">Redaction {redactionLabel(scope.policy)}</span>
                </p>
              </div>
            ))}
          </FramePanel>
        </Frame>
      </div>

      <Frame spacing="default" className="min-w-0">
        <FrameHeader className="flex flex-col gap-3">
          <div className="flex flex-col gap-0.5">
            <FrameTitle>Decision events</FrameTitle>
            <FrameDescription>
              Raw prompts, commands, arguments, and results are not shown.
            </FrameDescription>
          </div>
          <DecisionToolbar {...props} showPhase={false} />
        </FrameHeader>
        <FramePanel className="min-w-0 p-0">
          <DecisionGrid
            items={items}
            onSelect={setSelected}
            onEvaluator={(evaluator) => onSearch({ ...search, evaluator, page: undefined })}
          />
        </FramePanel>
        <FrameFooter>
          <DecisionPager {...props} />
        </FrameFooter>
      </Frame>
      <DecisionSheet decision={selected} onOpenChange={(open) => !open && setSelected(null)} />
    </div>
  )
}

function Kpi({
  label,
  value,
  hint,
  badge,
}: {
  label: string
  value: string
  hint: string
  badge?: 'destructive'
}) {
  return (
    <FramePanel className="flex flex-col gap-1">
      <span className="text-muted-foreground text-xs">{label}</span>
      <span className="flex items-center gap-2 text-2xl font-semibold tracking-tight tabular-nums">
        {value}
        {badge ? <Badge variant={badge}>Review</Badge> : null}
      </span>
      <span className="text-muted-foreground text-xs">{hint}</span>
    </FramePanel>
  )
}

function share(count = 0, total: number) {
  return total ? `${Math.round((count / total) * 100)}% of this page` : 'No decisions'
}

function DecisionGrid({
  items,
  onSelect,
  onEvaluator,
}: {
  items: GuardrailDecisionView[]
  onSelect: (decision: GuardrailDecisionView) => void
  onEvaluator: (evaluator: string) => void
}) {
  const [sorting, setSorting] = useState<SortingState>([])
  const columns = useMemo<ColumnDef<DataGridFeatures, GuardrailDecisionView>[]>(
    () => [
      {
        accessorKey: 'occurred_at',
        header: ({ column }) => <DataGridColumnHeader column={column} title="Time" />,
        size: 200,
        cell: ({ row }) => (
          <span className="text-muted-foreground tabular-nums">
            {formatTimestamp(row.original.occurred_at)}
          </span>
        ),
      },
      {
        accessorKey: 'action',
        header: 'Action',
        size: 120,
        cell: ({ row }) => (
          <Badge variant={actionVariant(row.original.action)}>
            {actionLabel(row.original.action)}
          </Badge>
        ),
      },
      {
        accessorKey: 'phase',
        header: 'Phase',
        size: 170,
        cell: ({ row }) => phaseLabel(row.original.phase),
      },
      {
        id: 'rule',
        header: 'Rule',
        size: 240,
        cell: ({ row }) => (
          <span className="flex min-w-0 flex-col">
            <span className="truncate">{row.original.reason_code}</span>
            <span className="text-muted-foreground truncate text-xs">
              {ruleLabel(row.original)}
            </span>
          </span>
        ),
      },
      {
        id: 'evaluator',
        header: 'Evaluator',
        size: 150,
        cell: ({ row }) => (
          <button
            type="button"
            title="Filter by this evaluator"
            className="hover:underline"
            onClick={(event) => {
              event.stopPropagation()
              onEvaluator(row.original.evaluator)
            }}
          >
            {evaluatorLabel(row.original)}
          </button>
        ),
      },
      {
        accessorKey: 'latency_micros',
        header: ({ column }) => <DataGridColumnHeader column={column} title="Latency" />,
        size: 110,
        cell: ({ row }) => (
          <span className="tabular-nums">{formatLatency(row.original.latency_micros)}</span>
        ),
      },
    ],
    [onEvaluator],
  )
  const table = useTable({
    features: dataGridFeatures,
    columns,
    data: items,
    getRowId: (decision: GuardrailDecisionView) => decision.decision_id,
    state: { sorting },
    onSortingChange: setSorting,
    manualPagination: true,
  })

  if (items.length === 0) {
    return (
      <Empty>
        <EmptyHeader>
          <EmptyTitle>No guardrail decisions</EmptyTitle>
          <EmptyDescription>No events match the current filters.</EmptyDescription>
        </EmptyHeader>
      </Empty>
    )
  }
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
