import { AdminTableHeader } from '@/components/shared/admin-table-header'
import { Badge } from '@/components/ui/badge'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
import { Table, TableBody, TableCell, TableRow } from '@/components/ui/table'
import { formatDuration } from '@/lib/format'
import type { RequestAttemptView } from '@/types/api'

import { attemptStatusBadgeVariant } from './format'

/** Upstream provider attempts in order, with any recorded error detail beneath the table. */
export function AttemptsTable({ attempts }: { attempts: RequestAttemptView[] | null | undefined }) {
  const recorded = attempts ?? []
  const failedAttempts: typeof recorded = []
  for (const attempt of recorded) {
    if (attempt.error_detail) failedAttempts.push(attempt)
  }

  if (recorded.length === 0) {
    return (
      <Empty className="border-border border">
        <EmptyHeader>
          <EmptyTitle>No provider attempts recorded</EmptyTitle>
          <EmptyDescription>
            This request log has no persisted upstream provider attempt rows.
          </EmptyDescription>
        </EmptyHeader>
      </Empty>
    )
  }

  return (
    <div className="flex min-w-0 flex-col gap-3">
      {/* The table scrolls inside its own outline when the sheet is narrow. */}
      <div className="border-border max-w-full overflow-x-auto rounded-lg border">
        <Table>
          <AdminTableHeader
            columns={[
              { label: 'Attempt', narrow: true },
              'Status',
              'Provider',
              'Upstream model',
              { label: 'Latency', align: 'right' },
              'Flags',
            ]}
          />
          <TableBody>
            {recorded.map((attempt) => (
              <TableRow key={attempt.request_attempt_id}>
                <TableCell className="px-3 font-mono tabular-nums">
                  #{attempt.attempt_number}
                </TableCell>
                <TableCell className="px-3">
                  <div className="flex items-center gap-2">
                    <Badge variant={attemptStatusBadgeVariant(attempt.status)}>
                      {attempt.status}
                    </Badge>
                    {attempt.error_code ? (
                      <span className="text-muted-foreground font-mono text-xs">
                        {attempt.error_code}
                      </span>
                    ) : null}
                  </div>
                </TableCell>
                <TableCell className="px-3 font-mono">{attempt.provider_key}</TableCell>
                <TableCell className="px-3 font-mono">{attempt.upstream_model}</TableCell>
                <TableCell className="px-3 text-right tabular-nums">
                  {formatDuration(attempt.latency_ms)}
                </TableCell>
                <TableCell className="px-3">
                  <div className="flex gap-1 whitespace-nowrap">
                    {attempt.retryable ? <Badge variant="outline">retryable</Badge> : null}
                    {attempt.terminal ? <Badge variant="outline">terminal</Badge> : null}
                    {attempt.produced_final_response ? (
                      <Badge variant="outline">final response</Badge>
                    ) : null}
                    {attempt.stream ? <Badge variant="outline">stream</Badge> : null}
                  </div>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
      {failedAttempts.map((attempt) => (
        <div
          key={`${attempt.request_attempt_id}-error`}
          className="border-destructive/40 min-w-0 border-l-2 py-1 pl-3"
        >
          <div className="flex flex-wrap items-center gap-2 text-sm font-medium">
            <span>Attempt #{attempt.attempt_number} error detail</span>
            {attempt.error_detail_truncated ? <Badge variant="warning">truncated</Badge> : null}
          </div>
          <p className="text-subtle-foreground mt-1 font-mono text-xs break-words">
            {attempt.error_detail}
          </p>
          <p className="text-muted-foreground mt-1 font-mono text-xs">route: {attempt.route_id}</p>
        </div>
      ))}
    </div>
  )
}
