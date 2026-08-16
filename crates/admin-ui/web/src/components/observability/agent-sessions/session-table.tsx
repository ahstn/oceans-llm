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
              {[
                'Started',
                'Harness',
                'Model',
                'State',
                'Score',
                'Cost',
                'Active time',
                'Requests',
                'Tool calls',
                'MCP calls',
                'Data quality',
              ].map((label) => (
                <th key={label} scope="col" className="h-9 px-3 font-semibold">
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
                  if (event.key === 'Enter' || event.key === ' ') onOpen(session)
                }}
              >
                <td className="px-3 py-2 whitespace-nowrap">
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
                <td className="px-3 py-2 tabular-nums">
                  <p className="font-medium">
                    {showScore ? (session.efficiency_score ?? '—') : 'Score not shown'}
                  </p>
                  <p className="text-muted-foreground text-xs">
                    {humanize(session.score_confidence)} confidence
                  </p>
                </td>
                <td className="px-3 py-2 tabular-nums">
                  {formatCost(session.normalized_cost_usd)}
                </td>
                <td className="px-3 py-2 tabular-nums">{formatDuration(session.active_time_ms)}</td>
                <td className="px-3 py-2 tabular-nums">{session.request_count.toLocaleString()}</td>
                <td className="px-3 py-2 tabular-nums">{formatCount(session.tool_call_count)}</td>
                <td className="px-3 py-2 tabular-nums">{formatCount(session.mcp_call_count)}</td>
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

const timestampFormatter = new Intl.DateTimeFormat('en-GB', {
  dateStyle: 'medium',
  timeStyle: 'short',
  timeZone: 'UTC',
})
const standardCurrency = new Intl.NumberFormat('en-GB', {
  style: 'currency',
  currency: 'USD',
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
})
const preciseCurrency = new Intl.NumberFormat('en-GB', {
  style: 'currency',
  currency: 'USD',
  minimumFractionDigits: 4,
  maximumFractionDigits: 4,
})

function formatTimestamp(value: string) {
  return timestampFormatter.format(new Date(value))
}
function formatCost(value?: number | null) {
  return value == null ? '—' : (value < 0.01 ? preciseCurrency : standardCurrency).format(value)
}
function formatDuration(value?: number | null) {
  if (value == null) return '—'
  if (value < 1_000) return `${value} ms`
  if (value < 60_000) return `${(value / 1_000).toFixed(1)} s`
  return `${(value / 60_000).toFixed(1)} min`
}
function formatCount(value?: number | null) {
  return value == null ? '—' : value.toLocaleString()
}
function humanize(value?: string | null) {
  return value
    ? value.replaceAll('_', ' ').replace(/\b\w/g, (character) => character.toUpperCase())
    : 'Unknown'
}
