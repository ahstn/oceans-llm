import type { ReactNode } from 'react'

import { CompactCount } from '@/components/data/compact-count'
import { AgentHarnessLabel } from '@/components/icons/agent-harness-icon'
import { AdminTableHead, AdminTableHeader } from '@/components/shared/admin-table-header'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Table, TableBody, TableCell, TableRow } from '@/components/ui/table'
import type { AgentSessionSummaryView } from '@/types/api'
import { EMPTY_VALUE, formatDuration, formatTimestamp, formatUsdTotal } from '@/lib/format'
import { cn } from '@/lib/utils'

import {
  sessionColumnIds,
  sessionColumnLabel,
  type SessionColumnId,
  type VisibleSessionColumns,
} from './session-columns'

interface SessionColumnRenderer {
  /** Numeric columns are right-aligned (DESIGN.md "Numbers and time"); their cells match. */
  numeric?: boolean
  /** The column's share of the table's minimum width, so hidden columns give width back. */
  minWidthRem: number
  cell: (session: AgentSessionSummaryView, showScore: boolean) => ReactNode
}

// A `Record` over every column ID, so adding a column to `session-columns.ts` requires a renderer.
const sessionColumnRenderers: Record<SessionColumnId, SessionColumnRenderer> = {
  started: {
    minWidthRem: 10,
    cell: (session) => formatTimestamp(session.started_at),
  },
  harness: {
    minWidthRem: 7,
    cell: (session) => <HarnessLabel session={session} />,
  },
  model: {
    minWidthRem: 11,
    cell: (session) => (
      <>
        <p>{session.requested_model_key}</p>
        <p className="text-muted-foreground text-xs">
          {session.operation} · {humanize(session.caller_class)}
        </p>
      </>
    ),
  },
  state: {
    minWidthRem: 5,
    cell: (session) => <StateBadge value={session.lifecycle} />,
  },
  score: {
    numeric: true,
    minWidthRem: 7,
    cell: (session, showScore) => <ScoreValue session={session} showScore={showScore} />,
  },
  cost: {
    numeric: true,
    minWidthRem: 4,
    cell: (session) => formatUsdTotal(session.normalized_cost_usd),
  },
  activeTime: {
    numeric: true,
    minWidthRem: 5,
    cell: (session) => formatDuration(session.active_time_ms),
  },
  requests: {
    numeric: true,
    minWidthRem: 4,
    cell: (session) => <CompactCount value={session.request_count} unit="requests" />,
  },
  toolCalls: {
    numeric: true,
    minWidthRem: 4,
    cell: (session) => <CompactCount value={session.tool_call_count} unit="tool calls" />,
  },
  mcpCalls: {
    numeric: true,
    minWidthRem: 4,
    cell: (session) => <CompactCount value={session.mcp_call_count} unit="MCP calls" />,
  },
  dataQuality: {
    minWidthRem: 6,
    cell: (session) => <DataQuality session={session} />,
  },
}

const sessionColumns = sessionColumnIds.map((key) => ({
  key,
  label: sessionColumnLabel(key),
  ...sessionColumnRenderers[key],
}))

interface SessionTableProps {
  items: AgentSessionSummaryView[]
  total: number
  page: number
  pageSize: number
  showScore: boolean
  loading: boolean
  visibleColumns: VisibleSessionColumns
  /** Offered beside the empty state, such as a button that clears the filters. */
  emptyAction?: ReactNode
  onOpen: (session: AgentSessionSummaryView) => void
  onPageChange: (page: number, pageSize: number) => void
}

/** Sessions as cards below `md` and as a table from `md` up, with their shared pager. */
export function SessionTable({
  items,
  total,
  page,
  pageSize,
  showScore,
  loading,
  visibleColumns,
  emptyAction,
  onOpen,
  onPageChange,
}: SessionTableProps) {
  const columns = sessionColumns.filter(
    (column) => column.key === 'started' || visibleColumns[column.key],
  )
  const minWidthRem = columns.reduce((width, column) => width + column.minWidthRem, 0)

  return (
    <div
      className={cn('flex min-w-0 flex-col gap-3', loading && 'opacity-60 transition-opacity')}
      aria-busy={loading}
    >
      <SessionCards items={items} showScore={showScore} emptyAction={emptyAction} onOpen={onOpen} />
      <div
        className="border-border hidden min-w-0 overflow-hidden rounded-md border md:block"
        data-testid="agent-session-table"
      >
        <Table className="text-sm" style={{ minWidth: `${minWidthRem}rem` }}>
          {/* Eleven columns fit a 1440 px card only with the tighter `px-2` cell padding. */}
          <AdminTableHeader>
            {columns.map((column) => (
              <AdminTableHead
                key={column.key}
                className={cn('px-2', column.numeric && 'text-right')}
              >
                {column.label}
              </AdminTableHead>
            ))}
          </AdminTableHeader>
          <TableBody>
            {items.map((session) => (
              <TableRow
                key={session.session_id}
                className="cursor-pointer"
                tabIndex={0}
                onClick={() => onOpen(session)}
                onKeyDown={(event) => {
                  // Keys pressed on a focusable child, such as a count's tooltip trigger, stay there.
                  if (event.target !== event.currentTarget) return
                  if (event.key === 'Enter' || event.key === ' ') onOpen(session)
                }}
              >
                {columns.map((column) => (
                  <TableCell
                    key={column.key}
                    className={cn(
                      'px-2 py-2',
                      column.numeric && 'text-right tabular-nums',
                      column.key === 'model' && 'whitespace-normal',
                      column.key === 'started' && 'tabular-nums',
                    )}
                  >
                    {column.cell(session, showScore)}
                  </TableCell>
                ))}
              </TableRow>
            ))}
          </TableBody>
        </Table>
        {items.length === 0 ? <EmptySessions action={emptyAction} /> : null}
      </div>
      <SessionPager total={total} page={page} pageSize={pageSize} onPageChange={onPageChange} />
    </div>
  )
}

/** Below `md`, each session is a card that opens its details from a button at its foot. */
function SessionCards({
  items,
  showScore,
  emptyAction,
  onOpen,
}: {
  items: AgentSessionSummaryView[]
  showScore: boolean
  emptyAction?: ReactNode
  onOpen: (session: AgentSessionSummaryView) => void
}) {
  return (
    <div className="grid grid-cols-1 gap-3 md:hidden" data-testid="agent-session-cards">
      {items.map((session) => (
        <article
          key={session.session_id}
          aria-label={`Session started ${formatTimestamp(session.started_at)}`}
          className="border-border bg-surface-muted flex min-w-0 flex-col gap-3 rounded-lg border p-4"
        >
          <div className="flex min-w-0 items-start justify-between gap-3">
            <div className="flex min-w-0 flex-col gap-0.5">
              <HarnessLabel session={session} />
              <p className="min-w-0 truncate text-sm">{session.requested_model_key}</p>
              <p className="text-muted-foreground text-xs whitespace-nowrap tabular-nums">
                {formatTimestamp(session.started_at)}
              </p>
            </div>
            <StateBadge value={session.lifecycle} />
          </div>
          <dl className="grid grid-cols-2 gap-x-4 gap-y-2 text-sm">
            <SessionFact label="Score">
              <ScoreValue session={session} showScore={showScore} />
            </SessionFact>
            <SessionFact label="Cost">{formatUsdTotal(session.normalized_cost_usd)}</SessionFact>
            <SessionFact label="Active time">{formatDuration(session.active_time_ms)}</SessionFact>
            <SessionFact label="Requests">
              <CompactCount value={session.request_count} unit="requests" />
            </SessionFact>
            <SessionFact label="Data quality">
              <DataQuality session={session} />
            </SessionFact>
          </dl>
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="w-full"
            onClick={() => onOpen(session)}
          >
            View session
          </Button>
        </article>
      ))}
      {items.length === 0 ? (
        <div className="border-border rounded-lg border">
          <EmptySessions action={emptyAction} />
        </div>
      ) : null}
    </div>
  )
}

function SessionFact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="min-w-0">
      <dt className="tracking-label text-muted-foreground text-xs font-semibold uppercase">
        {label}
      </dt>
      <dd className="mt-0.5 tabular-nums">{children}</dd>
    </div>
  )
}

function EmptySessions({ action }: { action?: ReactNode }) {
  return (
    <div className="flex flex-col items-center gap-3 px-4 py-10 text-center">
      <p className="text-muted-foreground text-sm">No agent sessions match these filters.</p>
      {action}
    </div>
  )
}

function SessionPager({
  total,
  page,
  pageSize,
  onPageChange,
}: {
  total: number
  page: number
  pageSize: number
  onPageChange: (page: number, pageSize: number) => void
}) {
  const pageCount = Math.max(1, Math.ceil(total / pageSize))
  const visiblePages = getVisiblePages(page, pageCount)
  const first = total === 0 ? 0 : (page - 1) * pageSize + 1
  const last = Math.min(page * pageSize, total)

  return (
    <div className="flex flex-wrap items-center justify-between gap-3 text-sm">
      <span className="text-muted-foreground tabular-nums">
        {first} - {last} of {total}
      </span>
      <div className="flex flex-wrap items-center gap-2">
        <Select value={String(pageSize)} onValueChange={(value) => onPageChange(1, Number(value))}>
          <SelectTrigger size="sm" aria-label="Rows per page">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {[25, 50, 100].map((size) => (
              <SelectItem key={size} value={String(size)}>
                {size}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Button
          variant="outline"
          size="sm"
          disabled={page <= 1}
          onClick={() => onPageChange(page - 1, pageSize)}
        >
          Previous
        </Button>
        {visiblePages.map((pageNumber) => (
          <Button
            key={pageNumber}
            variant={pageNumber === page ? 'secondary' : 'ghost'}
            size="sm"
            aria-label={`Go to page ${pageNumber}`}
            aria-current={pageNumber === page ? 'page' : undefined}
            onClick={() => onPageChange(pageNumber, pageSize)}
          >
            {pageNumber}
          </Button>
        ))}
        <Button
          variant="outline"
          size="sm"
          disabled={page >= pageCount}
          onClick={() => onPageChange(page + 1, pageSize)}
        >
          Next
        </Button>
      </div>
    </div>
  )
}

function getVisiblePages(page: number, pageCount: number) {
  const firstPage = Math.max(1, Math.min(page - 2, pageCount - 4))
  const lastPage = Math.min(pageCount, firstPage + 4)
  return Array.from({ length: lastPage - firstPage + 1 }, (_, index) => firstPage + index)
}

function HarnessLabel({ session }: { session: AgentSessionSummaryView }) {
  return (
    <AgentHarnessLabel harnessKey={session.harness_key ?? session.harness_label ?? ''}>
      {session.harness_label ?? session.harness_key ?? 'Unknown'}
    </AgentHarnessLabel>
  )
}

function ScoreValue({
  session,
  showScore,
}: {
  session: AgentSessionSummaryView
  showScore: boolean
}) {
  return (
    <>
      <p className="font-medium">
        {showScore ? (session.efficiency_score ?? EMPTY_VALUE) : 'Score not shown'}
      </p>
      <p className="text-muted-foreground text-xs">
        {humanize(session.score_confidence)} confidence
      </p>
    </>
  )
}

function DataQuality({ session }: { session: AgentSessionSummaryView }) {
  return session.limitations.length === 0 ? (
    <span className="text-muted-foreground">Complete</span>
  ) : (
    <Badge variant="outline">{session.limitations.length} data limits</Badge>
  )
}

function StateBadge({ value }: { value: string }) {
  return (
    <Badge variant={value.toLowerCase() === 'failed' ? 'destructive' : 'outline'}>
      {humanize(value)}
    </Badge>
  )
}

function humanize(value?: string | null) {
  return value
    ? value.replaceAll('_', ' ').replace(/\b\w/g, (character) => character.toUpperCase())
    : 'Unknown'
}
