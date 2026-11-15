import { AdminTableHeader, type AdminTableColumn } from '@/components/shared/admin-table-header'
import { Badge } from '@/components/ui/badge'
import { Table, TableBody, TableRow } from '@/components/ui/table'
import { EMPTY_VALUE, formatCount } from '@/lib/format'
import type { AttributionChangeView, AttributionPullRequestView } from '@/types/api'

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
import { pullRequestLabel, shortSha } from './shared'

const COLUMNS: AdminTableColumn[] = [
  'Commit',
  'Repository',
  'Branch',
  'Committed',
  'Author',
  'Pull request',
  { label: 'AI lines', align: 'right' },
  { label: 'Human lines', align: 'right' },
  'Sessions',
  { label: 'Requests', align: 'right' },
  { label: 'Cost', align: 'right' },
  'Models',
]

/** Commits newest first, with their lines, joined sessions, and the requests behind them. */
export function ChangesTable({
  changes,
  pullRequests,
}: {
  changes: AttributionChangeView[]
  /** The report's merged pull requests, which carry the URL a change's PR number links to. */
  pullRequests: AttributionPullRequestView[]
}) {
  const { visible, showMore } = useRowLimit(changes)
  if (changes.length === 0) {
    return (
      <p className="text-muted-foreground py-6 text-center text-sm">No commits in this window.</p>
    )
  }
  const pullRequestUrls = new Map(
    pullRequests.map((pullRequest) => [
      pullRequestLabel(pullRequest.repository, pullRequest.number),
      pullRequest.url,
    ]),
  )
  return (
    <div>
      <div className="border-border overflow-hidden rounded-md border">
        <Table aria-label="Recent changes" className="min-w-[84rem] text-left">
          <AdminTableHeader columns={COLUMNS} />
          <TableBody>
            {visible.map((change) => (
              <ChangeRow
                key={change.attribution_commit_id}
                change={change}
                pullRequestUrl={
                  change.pull_request_number == null
                    ? null
                    : pullRequestUrls.get(
                        pullRequestLabel(change.repository, change.pull_request_number),
                      )
                }
              />
            ))}
          </TableBody>
        </Table>
      </div>
      <RowLimitFooter
        shown={visible.length}
        total={changes.length}
        noun="changes"
        onShowMore={showMore}
      />
    </div>
  )
}

function ChangeRow({
  change,
  pullRequestUrl,
}: {
  change: AttributionChangeView
  pullRequestUrl: string | null | undefined
}) {
  // Gateway models come from the joined requests; without any, show the models the note names.
  const gatewayModels = change.models.length > 0
  return (
    <TableRow>
      <TextCell>
        <p className="font-mono text-xs" title={change.commit_sha}>
          {shortSha(change.commit_sha)}
        </p>
        {change.agent_tools.length > 0 ? (
          <p className="text-muted-foreground text-xs">{change.agent_tools.join(' · ')}</p>
        ) : null}
      </TextCell>
      <TextCell>
        <span className="block max-w-64 truncate font-mono text-xs" title={change.repository}>
          {change.repository}
        </span>
      </TextCell>
      <TextCell>
        {change.branch ? (
          <span className="inline-flex items-center gap-2">
            <span className="font-mono text-xs">{change.branch}</span>
            {change.is_default_branch ? <Badge variant="outline">Default</Badge> : null}
          </span>
        ) : (
          <span className="text-muted-foreground">{EMPTY_VALUE}</span>
        )}
      </TextCell>
      <TimestampCell value={change.committed_at} />
      <PersonCell
        person={change.author}
        fallback={
          <>
            <p className="text-muted-foreground">Unmatched author</p>
            <p className="text-muted-foreground text-xs">Key owner: {change.owner.name}</p>
          </>
        }
      />
      <TextCell>
        {change.pull_request_number == null ? (
          <span className="text-muted-foreground">{EMPTY_VALUE}</span>
        ) : (
          <PullRequestLink
            repository={change.repository}
            number={change.pull_request_number}
            url={pullRequestUrl}
            label={`#${change.pull_request_number}`}
          />
        )}
      </TextCell>
      <CountCell value={change.ai_lines} unit="AI lines" />
      <CountCell value={change.human_lines} unit="human lines" />
      <TextCell>
        <SessionJoinBadge change={change} />
      </TextCell>
      <CountCell value={change.request_count} unit="requests" />
      <CostCell
        cost={change.priced_cost_usd_10000}
        pricedRequests={change.priced_request_count}
        gaps={change.unpriced_request_count + change.usage_missing_request_count}
      />
      <ModelsCell
        models={gatewayModels ? change.models : change.note_models}
        muted={!gatewayModels}
      />
    </TableRow>
  )
}

/**
 * Whether the commit's AI sessions matched gateway traffic. Unjoined sessions still count their
 * lines but bring no requests or cost, so any unjoined session is a warning.
 */
function SessionJoinBadge({ change }: { change: AttributionChangeView }) {
  const joined = change.joined_session_count
  const unjoined = change.unjoined_session_count
  if (joined + unjoined === 0) {
    return <span className="text-muted-foreground">{EMPTY_VALUE}</span>
  }
  if (unjoined === 0) {
    return <Badge variant="success">{formatCount(joined)} joined</Badge>
  }
  return (
    <Badge variant="warning">
      {joined > 0 ? `${formatCount(joined)} joined · ` : null}
      {formatCount(unjoined)} unjoined
    </Badge>
  )
}
