import type { ReactNode } from 'react'

import { CompactCount } from '@/components/data/compact-count'
import { CallerCell, ModelCell } from '@/components/data/request-identity'
import { TablePagination } from '@/components/data/table-pagination'
import { AgentHarnessLabel } from '@/components/icons/agent-harness-icon'
import { AdminTableHead, AdminTableHeader } from '@/components/shared/admin-table-header'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Table, TableBody, TableCell, TableRow } from '@/components/ui/table'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { callerPrimary } from '@/lib/caller'
import type { AgentSessionSummaryView } from '@/types/api'
import {
  EMPTY_VALUE,
  formatCount,
  formatDuration,
  formatTimestamp,
  formatUsdTotal,
} from '@/lib/format'
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
  /** Shown as a labelled fact on the narrow-screen card; other columns sit in its header or not at all. */
  cardFact?: boolean
  cell: (session: AgentSessionSummaryView, showScore: boolean) => ReactNode
}

// A `Record` over every column ID, so adding a column to `session-columns.ts` requires a renderer.
const sessionColumnRenderers: Record<SessionColumnId, SessionColumnRenderer> = {
  started: {
    minWidthRem: 10,
    cell: (session) => formatTimestamp(session.started_at),
  },
  model: {
    minWidthRem: 11,
    cell: (session) => (
      <div className="max-w-60">
        <ModelCell
          modelKey={session.requested_model_key}
          modelIconKey={session.model_icon_key}
          providerKey={session.provider_key}
          providerIconKey={session.provider_icon_key}
        />
      </div>
    ),
  },
  caller: {
    minWidthRem: 10,
    cell: (session) => (
      <div className="max-w-56">
        <CallerCell caller={session} />
      </div>
    ),
  },
  harness: {
    minWidthRem: 7,
    cell: (session) => <HarnessLabel session={session} />,
  },
  state: {
    minWidthRem: 5,
    cell: (session) => <StateBadge value={session.lifecycle} />,
  },
  score: {
    cardFact: true,
    numeric: true,
    minWidthRem: 7,
    cell: (session, showScore) => <ScoreValue session={session} showScore={showScore} />,
  },
  cost: {
    cardFact: true,
    numeric: true,
    minWidthRem: 5,
    cell: (session) => <SessionCost session={session} />,
  },
  activeTime: {
    cardFact: true,
    numeric: true,
    minWidthRem: 5,
    cell: (session) => formatDuration(session.active_time_ms),
  },
  requests: {
    cardFact: true,
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
    cardFact: true,
    minWidthRem: 6,
    cell: (session) => <DataQuality session={session} />,
  },
}

const sessionColumns = sessionColumnIds.map((key) => ({
  key,
  label: sessionColumnLabel(key),
  ...sessionColumnRenderers[key],
}))

const sessionCardFacts = sessionColumns.filter((column) => column.cardFact)

const sessionPageSizes = [25, 50, 100] as const

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
  onPageChange: (page: number, pageSize: number, options?: { replace?: boolean }) => void
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
      <TablePagination
        total={total}
        page={page}
        pageSize={pageSize}
        pageSizes={sessionPageSizes}
        onPageChange={onPageChange}
      />
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
              {sessionColumnRenderers.harness.cell(session, showScore)}
              <p className="min-w-0 truncate text-sm">{session.requested_model_key}</p>
              <p className="text-muted-foreground min-w-0 truncate text-xs">
                {callerPrimary(session) ?? 'Unknown'}
              </p>
              <p className="text-muted-foreground text-xs whitespace-nowrap tabular-nums">
                {formatTimestamp(session.started_at)}
              </p>
            </div>
            {sessionColumnRenderers.state.cell(session, showScore)}
          </div>
          <dl className="grid grid-cols-2 gap-x-4 gap-y-2 text-sm">
            {sessionCardFacts.map((column) => (
              <SessionFact key={column.key} label={column.label}>
                {column.cell(session, showScore)}
              </SessionFact>
            ))}
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

/**
 * Ledger spend so far. When some requests have no price, the total leaves them out; that
 * exception lives in a tooltip (keyboard-reachable) and the accessible name.
 */
function SessionCost({ session }: { session: AgentSessionSummaryView }) {
  const amount = session.cost_usd_nanos == null ? null : session.cost_usd_nanos / 1_000_000_000
  const total = formatUsdTotal(amount)
  const unpriced = session.unpriced_request_count
  if (!unpriced) {
    return <>{total}</>
  }

  const note =
    unpriced === 1
      ? '1 request has no price, so the total leaves it out.'
      : `${formatCount(unpriced)} requests have no price, so the total leaves them out.`
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          tabIndex={0}
          aria-label={`${total}. ${note}`}
          className="focus-visible:ring-ring/50 decoration-muted-foreground cursor-default rounded-sm underline decoration-dotted underline-offset-4 outline-none focus-visible:ring-3"
          data-testid="agent-session-cost-unpriced"
        >
          {total}
        </span>
      </TooltipTrigger>
      <TooltipContent>{note}</TooltipContent>
    </Tooltip>
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
