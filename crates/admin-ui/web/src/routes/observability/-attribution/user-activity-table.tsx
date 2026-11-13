import { CompactCount } from '@/components/data/compact-count'
import { AdminTableHeader, type AdminTableColumn } from '@/components/shared/admin-table-header'
import { Table, TableBody, TableCell, TableRow } from '@/components/ui/table'
import { formatCount } from '@/lib/format'
import type { AttributionUserActivityView } from '@/types/api'

import { CostCell, CountCell, ModelsCell, PersonCell, TimestampCell } from './cells'

const COLUMNS: AdminTableColumn[] = [
  'User',
  { label: 'Commits', align: 'right' },
  { label: 'Merged PRs', align: 'right' },
  { label: 'AI lines', align: 'right' },
  { label: 'Human lines', align: 'right' },
  { label: 'AI sessions', align: 'right' },
  { label: 'Requests', align: 'right' },
  { label: 'Cost', align: 'right' },
  'Models',
  'Last activity',
]

/**
 * One row per Oceans user, matched by author email or else the key owner. With `onSelectUser`
 * (platform admins), each user's name is a toggle button that filters the report to that user.
 */
export function UserActivityTable({
  users,
  selectedUserId,
  onSelectUser,
}: {
  users: AttributionUserActivityView[]
  /** The report's user filter, echoed by the gateway. */
  selectedUserId: string | null | undefined
  onSelectUser?: (userId: string) => void
}) {
  if (users.length === 0) {
    return (
      <p className="text-muted-foreground py-6 text-center text-sm">
        No user activity in this window.
      </p>
    )
  }
  return (
    <div className="border-border overflow-hidden rounded-md border">
      <Table aria-label="User activity" className="min-w-[72rem] text-left">
        <AdminTableHeader columns={COLUMNS} />
        <TableBody>
          {users.map((row) => (
            <UserActivityRow
              key={row.user.id}
              row={row}
              selected={row.user.id === selectedUserId}
              onSelect={onSelectUser}
            />
          ))}
        </TableBody>
      </Table>
    </div>
  )
}

function UserActivityRow({
  row,
  selected,
  onSelect,
}: {
  row: AttributionUserActivityView
  selected: boolean
  onSelect?: (userId: string) => void
}) {
  return (
    <TableRow
      data-state={selected ? 'selected' : undefined}
      aria-current={selected ? 'true' : undefined}
    >
      {onSelect ? (
        <TableCell className="px-3 py-2">
          <button
            type="button"
            aria-pressed={selected}
            aria-label={`Filter the report to ${row.user.name}`}
            className="focus-visible:ring-ring rounded-sm text-left font-medium outline-none hover:underline focus-visible:ring-2"
            onClick={() => onSelect(row.user.id)}
          >
            {row.user.name}
          </button>
        </TableCell>
      ) : (
        <PersonCell person={row.user} fallback={null} />
      )}
      <CountCell value={row.commit_count} unit="commits" />
      <CountCell value={row.merged_pull_request_count} unit="merged pull requests" />
      <CountCell value={row.ai_lines} unit="AI lines" />
      <CountCell value={row.human_lines} unit="human lines" />
      <TableCell className="px-3 py-2 text-right tabular-nums">
        <p>
          <CompactCount value={row.ai_session_count} unit="AI sessions" />
        </p>
        <p className="text-muted-foreground text-xs">
          {formatCount(row.agent_session_count)} hooked
        </p>
      </TableCell>
      <CountCell value={row.request_count} unit="requests" />
      <CostCell
        cost={row.priced_cost_usd_10000}
        pricedRequests={row.priced_request_count}
        gaps={row.unpriced_request_count + row.usage_missing_request_count}
      />
      <ModelsCell models={row.models} />
      <TimestampCell value={row.last_activity_at} />
    </TableRow>
  )
}
