// Concept 4, Pull requests: Git AI's per-PR ledger with Oceans' cost join. Each row carries the AI
// share, a commit strip (one bar per commit, AI over human), the harnesses involved, cost and lead
// time. Expanding a row opens its timeline from first commit to merge, with the sessions behind it.

import { ArrowDown01Icon, ArrowRight01Icon } from '@hugeicons/core-free-icons'
import { Fragment, useMemo, useState, type CSSProperties } from 'react'

import { AppIcon } from '@/components/icons/app-icon'
import { AdminTableHead, AdminTableHeader } from '@/components/shared/admin-table-header'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Table, TableBody, TableCell, TableRow } from '@/components/ui/table'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { formatCount, formatTimestamp, formatUsd10000 } from '@/lib/format'

import { getMockDataset, mockUser, shortRepository, type MockCommit } from './fixtures'
import {
  AI_BANDS,
  aiBand,
  median,
  rollupPullRequest,
  sliceWindow,
  type AiBandKey,
  type ConceptWindow,
  type PullRequestRollup,
} from './metrics'
import {
  AI_TONE,
  AiShareBar,
  InsightCard,
  HUMAN_TONE,
  ReadinessBadge,
  Stat,
  Swatch,
  ToolLabel,
} from '@/components/attribution/parts'
import { formatHours, formatPercent } from '@/components/attribution/format'

type Sort = 'recent' | 'cost' | 'ai'

// A concept page is one linear layout read top to bottom; splitting it would scatter the mockup.
// oxlint-disable-next-line eslint/max-lines-per-function
export default function PullRequestsConcept({ days }: { days: ConceptWindow }) {
  const [band, setBand] = useState<AiBandKey | 'all'>('all')
  const [sort, setSort] = useState<Sort>('recent')
  const [expanded, setExpanded] = useState<string | null>(null)

  const rows = useMemo(() => {
    const dataset = getMockDataset()
    const slice = sliceWindow(dataset, days)
    return slice.pullRequests.map((pr) => rollupPullRequest(slice, pr))
  }, [days])

  const visible = rows
    .filter((row) => band === 'all' || aiBand(row.aiShare) === band)
    .sort((a, b) =>
      sort === 'cost'
        ? b.costUsd10000 - a.costUsd10000
        : sort === 'ai'
          ? b.aiShare - a.aiShare
          : (b.pr.mergedAt ?? b.pr.openedAt) - (a.pr.mergedAt ?? a.pr.openedAt),
    )
    .slice(0, 25)
  const merged = rows.filter((row) => row.pr.state === 'merged')

  return (
    <div className="flex flex-col gap-6">
      <InsightCard
        title="Pull requests"
        description="Every PR in the window with the attribution and gateway cost of its commits. Expand a row for its timeline."
        readiness="live"
        readinessNote="Rows are live today; lead time, state and durability are additions."
        action={
          <ToggleGroup
            type="single"
            variant="outline"
            size="sm"
            value={sort}
            onValueChange={(next) => next && setSort(next as Sort)}
            aria-label="Sort pull requests"
          >
            <ToggleGroupItem value="recent">Recent</ToggleGroupItem>
            <ToggleGroupItem value="cost">Cost</ToggleGroupItem>
            <ToggleGroupItem value="ai">AI share</ToggleGroupItem>
          </ToggleGroup>
        }
      >
        <div className="flex flex-col gap-4">
          <div className="flex flex-wrap items-end justify-between gap-4">
            <div className="grid grid-cols-2 gap-6 sm:grid-cols-4">
              <Stat label="Merged" value={formatCount(merged.length)} />
              <Stat
                label="Median AI share"
                value={formatPercent(median(merged.map((row) => row.aiShare)))}
              />
              <Stat
                label="Median cost"
                value={formatUsd10000(median(merged.map((row) => row.costUsd10000)) ?? 0)}
              />
              <Stat
                label="Median lead time"
                value={formatHours(
                  median(merged.flatMap((row) => (row.leadTimeMs ? [row.leadTimeMs] : []))),
                )}
              />
            </div>
            <ToggleGroup
              type="single"
              variant="outline"
              size="sm"
              value={band}
              onValueChange={(next) => next && setBand(next as AiBandKey | 'all')}
              aria-label="Filter by AI share"
            >
              <ToggleGroupItem value="all">All</ToggleGroupItem>
              {AI_BANDS.map((entry) => (
                <ToggleGroupItem key={entry.key} value={entry.key}>
                  {entry.label}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
          </div>

          <div className="border-border overflow-x-auto rounded-md border">
            <Table aria-label="Pull requests" className="min-w-[60rem]">
              <AdminTableHeader>
                {['Pull request', 'State', 'AI share', 'Commits', 'Harnesses'].map((label) => (
                  <AdminTableHead key={label}>{label}</AdminTableHead>
                ))}
                {['Cost', 'Lead time', 'Durable 30d'].map((label) => (
                  <AdminTableHead key={label} className="text-right">
                    {label}
                  </AdminTableHead>
                ))}
                {/* Pinned right, so the details stay in reach while the ledger scrolls sideways. */}
                <AdminTableHead
                  className="bg-surface-muted shadow-sticky-edge-end sticky right-0 z-10 w-10"
                  aria-label="Details"
                />
              </AdminTableHeader>
              <TableBody>
                {visible.map((row) => {
                  const key = `${row.pr.repository}#${row.pr.number}`
                  const open = expanded === key
                  return (
                    <Fragment key={key}>
                      <TableRow
                        className="group hover:bg-surface-muted cursor-pointer"
                        data-state={open ? 'selected' : undefined}
                        onClick={() => setExpanded(open ? null : key)}
                      >
                        <TableCell className="px-3 py-2">
                          <p className="max-w-80 truncate font-medium">{row.pr.title}</p>
                          <p className="text-muted-foreground text-xs">
                            <span className="font-mono">
                              {shortRepository(row.pr.repository)}#{row.pr.number}
                            </span>{' '}
                            · {mockUser(row.pr.userId).name}
                          </p>
                        </TableCell>
                        <TableCell className="px-3 py-2">
                          <StateBadge row={row} />
                        </TableCell>
                        <TableCell className="w-40 px-3 py-2">
                          <div className="flex items-center gap-2">
                            <div className="flex-1">
                              <AiShareBar
                                ai={row.aiLines}
                                human={row.humanLines}
                                unknown={row.unknownLines}
                                size="sm"
                              />
                            </div>
                            <span className="w-9 text-right text-xs tabular-nums">
                              {formatPercent(row.aiShare)}
                            </span>
                          </div>
                        </TableCell>
                        <TableCell className="px-3 py-2">
                          <CommitStrip commits={row.commits} />
                        </TableCell>
                        <TableCell className="px-3 py-2">
                          <span className="flex items-center gap-2">
                            {row.tools.length === 0 ? (
                              <span className="text-muted-foreground">—</span>
                            ) : null}
                            {row.tools.map((tool) => (
                              <ToolLabel key={tool} harness={tool} hideName />
                            ))}
                          </span>
                        </TableCell>
                        <TableCell className="px-3 py-2 text-right tabular-nums">
                          {row.costUsd10000 > 0 ? formatUsd10000(row.costUsd10000) : '—'}
                        </TableCell>
                        <TableCell className="px-3 py-2 text-right tabular-nums">
                          {formatHours(row.leadTimeMs)}
                        </TableCell>
                        <TableCell className="px-3 py-2 text-right tabular-nums">
                          {row.pr.state === 'merged' && row.aiLines > 0
                            ? formatPercent(row.pr.aiLinesDurable30 / row.aiLines)
                            : '—'}
                        </TableCell>
                        <TableCell className="bg-card shadow-sticky-edge-end group-hover:bg-surface-muted group-data-[state=selected]:bg-muted sticky right-0 z-10 w-10 px-2 py-2">
                          <Button
                            variant="ghost"
                            size="icon-sm"
                            aria-expanded={open}
                            aria-label={
                              open ? `Collapse #${row.pr.number}` : `Expand #${row.pr.number}`
                            }
                          >
                            <AppIcon
                              icon={open ? ArrowDown01Icon : ArrowRight01Icon}
                              size={16}
                              aria-hidden
                            />
                          </Button>
                        </TableCell>
                      </TableRow>
                      {open ? (
                        <TableRow className="hover:bg-transparent">
                          <TableCell colSpan={9} className="bg-muted/40 p-0">
                            <PullRequestDetail row={row} />
                          </TableCell>
                        </TableRow>
                      ) : null}
                    </Fragment>
                  )
                })}
              </TableBody>
            </Table>
          </div>
          <div className="text-muted-foreground flex flex-wrap items-center justify-between gap-3 text-xs">
            <span className="flex flex-wrap items-center gap-3">
              <Swatch tone={AI_TONE} label="AI lines" />
              <Swatch tone={HUMAN_TONE} label="Human lines" />
              <span className="inline-flex items-center gap-1.5">
                30-day durability <ReadinessBadge level="new" />
              </span>
            </span>
            <span>
              Showing {visible.length} of {rows.length}
            </span>
          </div>
        </div>
      </InsightCard>
    </div>
  )
}

function StateBadge({ row }: { row: PullRequestRollup }) {
  if (row.pr.reverted) return <Badge variant="destructive">Reverted</Badge>
  if (row.pr.state === 'merged') return <Badge variant="success">Merged</Badge>
  if (row.pr.state === 'open') return <Badge variant="warning">In review</Badge>
  return <Badge variant="secondary">Closed</Badge>
}

/** One bar per commit, height by added lines, AI stacked over human. */
export function CommitStrip({ commits }: { commits: MockCommit[] }) {
  const max = Math.max(1, ...commits.map((commit) => commit.aiLines + commit.humanLines))
  return (
    <div className="flex h-7 items-end gap-0.5" role="img" aria-label={`${commits.length} commits`}>
      {commits.slice(0, 16).map((commit) => {
        const total = commit.aiLines + commit.humanLines
        const height = Math.max(3, (total / max) * 28)
        return (
          <span
            key={commit.sha}
            title={`${commit.sha.slice(0, 7)}: ${commit.aiLines} AI, ${commit.humanLines} human`}
            className="flex w-1.5 flex-col justify-end overflow-hidden rounded-sm"
            style={{ height }}
          >
            <span
              className="bg-(--ai)"
              style={
                {
                  '--ai': AI_TONE,
                  height: `${(commit.aiLines / Math.max(1, total)) * 100}%`,
                } as CSSProperties
              }
            />
            <span
              className="flex-1 bg-(--human)"
              style={{ '--human': HUMAN_TONE } as CSSProperties}
            />
          </span>
        )
      })}
    </div>
  )
}

/** First commit → commits → PR opened → merged, on one time axis, with the per-commit join. */
function PullRequestDetail({ row }: { row: PullRequestRollup }) {
  const start = row.pr.firstCommitAt
  const end =
    row.pr.mergedAt ?? Math.max(row.pr.openedAt, ...row.commits.map((commit) => commit.at))
  const span = Math.max(1, end - start)
  const position = (at: number) => `${((at - start) / span) * 100}%`
  return (
    <div className="flex flex-col gap-4 p-4">
      <div className="flex flex-col gap-2">
        <div className="text-muted-foreground flex justify-between text-xs">
          <span>First commit · {formatTimestamp(new Date(start))}</span>
          <span>
            {row.pr.mergedAt ? 'Merged' : 'Now'} · {formatTimestamp(new Date(end))}
          </span>
        </div>
        <div className="relative h-8">
          <div className="bg-border absolute inset-x-0 top-1/2 h-px" />
          <div
            className="bg-chart-hero/25 absolute top-1/2 h-2 -translate-y-1/2 rounded-full"
            style={{ left: position(row.pr.openedAt), right: 0 }}
            title="In review"
          />
          {row.commits.map((commit) => (
            <span
              key={commit.sha}
              className="border-card absolute top-1/2 size-3 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 bg-(--dot)"
              style={
                {
                  left: position(commit.at),
                  '--dot': commit.aiLines > commit.humanLines ? AI_TONE : HUMAN_TONE,
                } as CSSProperties
              }
              title={`${commit.sha.slice(0, 7)} · ${formatTimestamp(new Date(commit.at))}`}
            />
          ))}
          <span
            className="bg-foreground absolute top-1/2 h-4 w-px -translate-y-1/2"
            style={{ left: position(row.pr.openedAt) }}
            title="PR opened"
          />
        </div>
        <div className="text-muted-foreground flex flex-wrap gap-4 text-xs">
          <span>Writing: {formatHours(row.pr.openedAt - start)}</span>
          <span className="inline-flex items-center gap-1.5">
            In review: {formatHours(end - row.pr.openedAt)}{' '}
            <ReadinessBadge level="new" note="Needs PR opened_at." />
          </span>
          <span>{formatCount(row.pr.reviewComments)} review comments</span>
          <span>{formatCount(row.pr.aiLinesReworkedInReview)} AI lines reworked in review</span>
        </div>
      </div>
      <div className="border-border overflow-hidden rounded-md border">
        <Table aria-label={`Commits in #${row.pr.number}`}>
          <AdminTableHeader
            columns={[
              'Commit',
              'Committed',
              'Harness',
              'Model',
              { label: 'AI lines', align: 'right' },
              { label: 'Human lines', align: 'right' },
              { label: 'Requests', align: 'right' },
              { label: 'Cost', align: 'right' },
            ]}
          />
          <TableBody>
            {row.commits.map((commit) => (
              <TableRow key={commit.sha}>
                <TableCell className="px-3 py-1.5 font-mono text-xs">
                  {commit.sha.slice(0, 7)}
                </TableCell>
                <TableCell className="px-3 py-1.5 whitespace-nowrap">
                  {formatTimestamp(new Date(commit.at))}
                </TableCell>
                <TableCell className="px-3 py-1.5">
                  {commit.tool ? <ToolLabel harness={commit.tool} /> : '—'}
                </TableCell>
                <TableCell className="px-3 py-1.5 font-mono text-xs">
                  {commit.model ?? '—'}
                </TableCell>
                <TableCell className="px-3 py-1.5 text-right tabular-nums">
                  {formatCount(commit.aiLines)}
                </TableCell>
                <TableCell className="px-3 py-1.5 text-right tabular-nums">
                  {formatCount(commit.humanLines)}
                </TableCell>
                <TableCell className="px-3 py-1.5 text-right tabular-nums">
                  {formatCount(commit.requests)}
                </TableCell>
                <TableCell className="px-3 py-1.5 text-right tabular-nums">
                  {commit.costUsd10000 > 0 ? formatUsd10000(commit.costUsd10000) : '—'}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </div>
  )
}
