import { CompactCount } from '@/components/data/compact-count'
import { AgentHarnessLabel } from '@/components/icons/agent-harness-icon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import type { AgentSessionSummaryView } from '@/types/api'
import { EMPTY_VALUE, formatDuration, formatTimestamp, formatUsdTotal } from '@/lib/format'
import { cn } from '@/lib/utils'

interface SessionTableProps {
  items: AgentSessionSummaryView[]
  total: number
  page: number
  pageSize: number
  showScore: boolean
  loading: boolean
  onOpen: (session: AgentSessionSummaryView) => void
  onPageChange: (page: number, pageSize: number) => void
}

// Keep the session rows and their tightly coupled pagination controls in one table component.
// oxlint-disable-next-line eslint/max-lines-per-function
export function SessionTable({
  items,
  total,
  page,
  pageSize,
  showScore,
  loading,
  onOpen,
  onPageChange,
}: SessionTableProps) {
  const pageCount = Math.max(1, Math.ceil(total / pageSize))
  const visiblePages = getVisiblePages(page, pageCount)
  const first = total === 0 ? 0 : (page - 1) * pageSize + 1
  const last = Math.min(page * pageSize, total)

  return (
    <div className={loading ? 'opacity-60 transition-opacity' : undefined} aria-busy={loading}>
      <div className="min-w-0 overflow-x-auto rounded-md border">
        <table className="w-full min-w-[960px] text-sm">
          <thead className="bg-muted/40 text-muted-foreground text-left">
            <tr>
              {sessionColumns.map(({ label, numeric }) => (
                <th
                  key={label}
                  scope="col"
                  className={cn('h-9 px-3 font-semibold', numeric && 'text-right')}
                >
                  {label}
                </th>
              ))}
            </tr>
          </thead>
          <tbody className="divide-y">
            {items.map((session) => (
              <tr
                key={session.session_id}
                className="hover:bg-muted/40 cursor-pointer transition-colors"
                tabIndex={0}
                onClick={() => onOpen(session)}
                onKeyDown={(event) => {
                  // Keys pressed on a focusable child, such as a count's tooltip trigger, stay there.
                  if (event.target !== event.currentTarget) return
                  if (event.key === 'Enter' || event.key === ' ') onOpen(session)
                }}
              >
                <td className="px-3 py-2 whitespace-nowrap tabular-nums">
                  {formatTimestamp(session.started_at)}
                </td>
                <td className="px-3 py-2">
                  <AgentHarnessLabel
                    harnessKey={session.harness_key ?? session.harness_label ?? ''}
                  >
                    {session.harness_label ?? session.harness_key ?? 'Unknown'}
                  </AgentHarnessLabel>
                </td>
                <td className="px-3 py-2">
                  <p>{session.requested_model_key}</p>
                  <p className="text-muted-foreground text-xs">
                    {session.operation} · {humanize(session.caller_class)}
                  </p>
                </td>
                <td className="px-3 py-2">
                  <StateBadge value={session.lifecycle} />
                </td>
                <td className="px-3 py-2 text-right tabular-nums">
                  <p className="font-medium">
                    {showScore ? (session.efficiency_score ?? EMPTY_VALUE) : 'Score not shown'}
                  </p>
                  <p className="text-muted-foreground text-xs">
                    {humanize(session.score_confidence)} confidence
                  </p>
                </td>
                <td className="px-3 py-2 text-right tabular-nums">
                  {formatUsdTotal(session.normalized_cost_usd)}
                </td>
                <td className="px-3 py-2 text-right whitespace-nowrap tabular-nums">
                  {formatDuration(session.active_time_ms)}
                </td>
                <td className="px-3 py-2 text-right tabular-nums">
                  <CompactCount value={session.request_count} unit="requests" />
                </td>
                <td className="px-3 py-2 text-right tabular-nums">
                  <CompactCount value={session.tool_call_count} unit="tool calls" />
                </td>
                <td className="px-3 py-2 text-right tabular-nums">
                  <CompactCount value={session.mcp_call_count} unit="MCP calls" />
                </td>
                <td className="px-3 py-2">
                  {session.limitations.length === 0 ? (
                    <span className="text-muted-foreground">Complete</span>
                  ) : (
                    <Badge variant="outline">{session.limitations.length} data limits</Badge>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {items.length === 0 ? (
          <p className="text-muted-foreground px-4 py-10 text-center text-sm">
            No agent sessions match these filters.
          </p>
        ) : null}
      </div>
      <div className="mt-3 flex flex-wrap items-center justify-between gap-3 text-sm">
        <span className="text-muted-foreground tabular-nums">
          {first} - {last} of {total}
        </span>
        <div className="flex items-center gap-2">
          <Select
            value={String(pageSize)}
            onValueChange={(value) => onPageChange(1, Number(value))}
          >
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
    </div>
  )
}

function getVisiblePages(page: number, pageCount: number) {
  const firstPage = Math.max(1, Math.min(page - 2, pageCount - 4))
  const lastPage = Math.min(pageCount, firstPage + 4)
  return Array.from({ length: lastPage - firstPage + 1 }, (_, index) => firstPage + index)
}

function StateBadge({ value }: { value: string }) {
  return (
    <Badge variant={value.toLowerCase() === 'failed' ? 'destructive' : 'outline'}>
      {humanize(value)}
    </Badge>
  )
}

// Numeric columns are right-aligned (DESIGN.md "Numbers and time"); their cells match.
const sessionColumns = [
  { label: 'Started', numeric: false },
  { label: 'Harness', numeric: false },
  { label: 'Model', numeric: false },
  { label: 'State', numeric: false },
  { label: 'Score', numeric: true },
  { label: 'Cost', numeric: true },
  { label: 'Active time', numeric: true },
  { label: 'Requests', numeric: true },
  { label: 'Tool calls', numeric: true },
  { label: 'MCP calls', numeric: true },
  { label: 'Data quality', numeric: false },
] as const

function humanize(value?: string | null) {
  return value
    ? value.replaceAll('_', ' ').replace(/\b\w/g, (character) => character.toUpperCase())
    : 'Unknown'
}
