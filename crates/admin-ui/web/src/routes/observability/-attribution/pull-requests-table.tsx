import { AdminTableHeader, type AdminTableColumn } from '@/components/shared/admin-table-header'
import { Table, TableBody, TableCell, TableRow } from '@/components/ui/table'
import { formatCount } from '@/lib/format'
import type { AttributionPullRequestView } from '@/types/api'

import {
  CostCell,
  CountCell,
  ModelsCell,
  PersonCell,
  PullRequestLink,
  TextCell,
  TimestampCell,
} from './cells'
import { RowLimitFooter, useRowLimit } from './row-limit'

const COLUMNS: AdminTableColumn[] = [
  'Pull request',
  'Title',
  'Merged',
  'Author',
  { label: 'Commits', align: 'right' },
  { label: 'AI lines', align: 'right' },
  { label: 'Human lines', align: 'right' },
  { label: 'Requests', align: 'right' },
  { label: 'Cost', align: 'right' },
  'Models',
]

/** Merged pull requests, newest first, with the attribution of their original commits. */
export function PullRequestsTable({
  pullRequests,
}: {
  pullRequests: AttributionPullRequestView[]
}) {
  const { visible, showMore } = useRowLimit(pullRequests)
  if (pullRequests.length === 0) {
    return (
      <p className="text-muted-foreground py-6 text-center text-sm">
        No merged pull requests in this window.
      </p>
    )
  }
  return (
    <div>
      <div className="border-border overflow-hidden rounded-md border">
        <Table aria-label="Merged pull requests" className="min-w-[76rem] text-left">
          <AdminTableHeader columns={COLUMNS} />
          <TableBody>
            {visible.map((pullRequest) => (
              <TableRow key={`${pullRequest.repository}#${pullRequest.number}`}>
                <TextCell>
                  <PullRequestLink
                    repository={pullRequest.repository}
                    number={pullRequest.number}
                    url={pullRequest.url}
                  />
                </TextCell>
                <TextCell>
                  <p className="max-w-80 truncate" title={pullRequest.title}>
                    {pullRequest.title}
                  </p>
                  {pullRequest.head_ref && pullRequest.base_ref ? (
                    <p className="text-muted-foreground font-mono text-xs">
                      {pullRequest.head_ref} → {pullRequest.base_ref}
                    </p>
                  ) : null}
                </TextCell>
                <TimestampCell value={pullRequest.merged_at} />
                <PersonCell
                  person={pullRequest.author}
                  fallback={<span className="text-muted-foreground">Unmatched author</span>}
                />
                <TableCell className="px-3 py-2 text-right tabular-nums">
                  <p>{formatCount(pullRequest.commit_count)}</p>
                  <p className="text-muted-foreground text-xs">
                    {formatCount(pullRequest.attributed_commit_count)} with notes
                  </p>
                </TableCell>
                <CountCell value={pullRequest.ai_lines} unit="AI lines" />
                <CountCell value={pullRequest.human_lines} unit="human lines" />
                <CountCell value={pullRequest.request_count} unit="requests" />
                <CostCell
                  cost={pullRequest.priced_cost_usd_10000}
                  pricedRequests={pullRequest.priced_request_count}
                  gaps={
                    pullRequest.unpriced_request_count + pullRequest.usage_missing_request_count
                  }
                />
                <ModelsCell models={pullRequest.models} />
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
      <RowLimitFooter
        shown={visible.length}
        total={pullRequests.length}
        noun="pull requests"
        onShowMore={showMore}
      />
    </div>
  )
}
