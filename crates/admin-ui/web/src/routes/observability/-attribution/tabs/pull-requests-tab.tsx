// Pull requests: a ledger of merged pull requests with each one's AI share, a commit strip, the
// harnesses behind it, cost and lead time. Expanding a row shows its timeline and per-commit join.
// Recent changes, every attributed commit in the window, sits below.

import { ArrowDown01Icon, ArrowRight01Icon } from '@hugeicons/core-free-icons'
import { Fragment, useState, type CSSProperties } from 'react'

import {
  AI_TONE,
  AiShareBar,
  HUMAN_TONE,
  InsightCard,
  ReadinessBadge,
  Stat,
  Swatch,
  ToolLabel,
} from '@/components/attribution/parts'
import { formatHours, formatPercent } from '@/components/attribution/format'
import { AppIcon } from '@/components/icons/app-icon'
import { AdminTableHead, AdminTableHeader } from '@/components/shared/admin-table-header'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Table, TableBody, TableCell, TableRow } from '@/components/ui/table'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { EMPTY_VALUE, formatCount, formatTimestamp, formatUsd10000 } from '@/lib/format'
import type { AttributionReportView } from '@/types/api'

import { formatAttributedCost, shortSha } from '../shared'
import { PullRequestLink } from '../cells'
import { displayRepository } from '../shared'
import { ChangesTable } from '../changes-table'
import { AI_BANDS, median, pullRequestRows, type AiBandKey, type PullRequestRow } from '../insights'
import { RowLimitFooter, useRowLimit } from '../row-limit'

type Sort = 'recent' | 'cost' | 'ai'
type CommitView = AttributionReportView['pull_requests'][number]['commits'][number]

// A tab is one linear layout read top to bottom; splitting it would scatter the sections.
// oxlint-disable-next-line eslint/max-lines-per-function
export default function PullRequestsTab({ report }: { report: AttributionReportView }) {
  const [band, setBand] = useState<AiBandKey | 'all'>('all')
  const [sort, setSort] = useState<Sort>('recent')
  const [expanded, setExpanded] = useState<string | null>(null)
  const rows = pullRequestRows(report.pull_requests)
  const filtered = rows
    .filter((row) => band === 'all' || row.band === band)
    .sort((a, b) =>
      sort === 'cost'
        ? b.pr.priced_cost_usd_10000 - a.pr.priced_cost_usd_10000
        : sort === 'ai'
          ? (b.aiShare ?? -1) - (a.aiShare ?? -1)
          : b.pr.merged_at.localeCompare(a.pr.merged_at),
    )
  const { visible, showMore } = useRowLimit(filtered)
  const priced = rows.filter((row) => row.pr.priced_request_count > 0)

  return (
    <div className="flex flex-col gap-6">
      <InsightCard
        title="Merged pull requests"
        description={
          <>
            Pull requests reported by <code className="font-mono">oceans hook prs</code>, with the
            attribution and gateway cost of their original commits. Expand a row for its timeline.
          </>
        }
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
        {rows.length === 0 ? (
          <p className="text-muted-foreground py-6 text-center text-sm">
            No merged pull requests in this window.
          </p>
        ) : (
          <div className="flex flex-col gap-4">
            <div className="flex flex-wrap items-end justify-between gap-4">
              <div className="grid grid-cols-2 gap-6 sm:grid-cols-4">
                <Stat label="Merged" value={formatCount(rows.length)} />
                <Stat
                  label="Median AI share"
                  value={formatPercent(
                    median(rows.flatMap((row) => (row.aiShare == null ? [] : [row.aiShare]))),
                  )}
                />
                <Stat
                  label="Median cost"
                  value={formatUsd10000OrEmpty(
                    median(priced.map((row) => row.pr.priced_cost_usd_10000)),
                  )}
                />
                <Stat
                  label="Median lead time"
                  value={formatHours(
                    toMs(
                      median(rows.flatMap((row) => (row.leadHours == null ? [] : [row.leadHours]))),
                    ),
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
              <Table aria-label="Merged pull requests" className="min-w-[60rem]">
                <AdminTableHeader>
                  <AdminTableHead>Pull request</AdminTableHead>
                  <AdminTableHead>AI share</AdminTableHead>
                  <AdminTableHead>Commits</AdminTableHead>
                  <AdminTableHead>Harnesses</AdminTableHead>
                  <AdminTableHead className="text-right">Cost</AdminTableHead>
                  <AdminTableHead className="text-right">Lead time</AdminTableHead>
                  <AdminTableHead className="text-right">
                    <span className="inline-flex items-center gap-1.5">
                      Durable 30d
                      <ReadinessBadge
                        level="new"
                        note="Needs line survival after merge, from scheduled git ai blame runs."
                      />
                    </span>
                  </AdminTableHead>
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
                        <PullRequestRowView
                          row={row}
                          open={open}
                          onToggle={() => setExpanded(open ? null : key)}
                        />
                        {open ? (
                          <TableRow className="hover:bg-transparent">
                            <TableCell colSpan={8} className="bg-muted/40 p-0">
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
            <div className="text-muted-foreground flex flex-wrap items-center gap-3 text-xs">
              <Swatch tone={AI_TONE} label="AI lines" />
              <Swatch tone={HUMAN_TONE} label="Human lines" />
            </div>
            <RowLimitFooter
              shown={visible.length}
              total={filtered.length}
              noun="pull requests"
              onShowMore={showMore}
            />
          </div>
        )}
      </InsightCard>

      <InsightCard
        title="Recent changes"
        description="Each commit gets the requests its AI sessions made since their previous commit. Sessions with no matching gateway traffic are unjoined: their lines count, but they bring no requests or cost."
      >
        <ChangesTable changes={report.changes} pullRequests={report.pull_requests} />
      </InsightCard>
    </div>
  )
}

function toMs(hours: number | null) {
  return hours == null ? null : hours * 3_600_000
}

function formatUsd10000OrEmpty(value: number | null) {
  return value == null ? EMPTY_VALUE : formatUsd10000(value)
}

function PullRequestRowView({
  row,
  open,
  onToggle,
}: {
  row: PullRequestRow
  open: boolean
  onToggle: () => void
}) {
  const { pr } = row
  const gaps = pr.unpriced_request_count + pr.usage_missing_request_count
  return (
    // The whole row toggles its details; the button and link inside stop their clicks reaching it.
    <TableRow
      className="group hover:bg-surface-muted cursor-pointer"
      data-state={open ? 'selected' : undefined}
      onClick={onToggle}
    >
      <TableCell className="px-3 py-2">
        <p className="flex max-w-96 items-center gap-2">
          <span className="truncate font-medium" title={pr.title}>
            {pr.title}
          </span>
          {pr.is_revert ? <Badge variant="warning">Revert</Badge> : null}
        </p>
        <p className="text-muted-foreground flex flex-wrap items-center gap-x-1 text-xs">
          <span onClick={(event) => event.stopPropagation()}>
            <PullRequestLink
              repository={pr.repository}
              number={pr.number}
              url={pr.url}
              label={`${displayRepository(pr.repository)}#${pr.number}`}
            />
          </span>
          <span>· {pr.author?.name ?? 'Unmatched author'}</span>
        </p>
      </TableCell>
      <TableCell className="w-44 px-3 py-2">
        {row.added > 0 ? (
          <div className="flex items-center gap-2">
            <div className="flex-1">
              <AiShareBar ai={pr.ai_lines} human={pr.human_lines} size="sm" />
            </div>
            <span className="w-9 text-right text-xs tabular-nums">
              {formatPercent(row.aiShare)}
            </span>
          </div>
        ) : (
          <span className="text-muted-foreground text-xs">No notes</span>
        )}
      </TableCell>
      <TableCell className="px-3 py-2">
        <CommitStrip
          commits={pr.commits}
          attributed={pr.attributed_commit_count}
          total={pr.commit_count}
        />
      </TableCell>
      <TableCell className="px-3 py-2">
        <span className="flex items-center gap-2">
          {pr.agent_tools.length === 0 ? (
            <span className="text-muted-foreground">{EMPTY_VALUE}</span>
          ) : null}
          {pr.agent_tools.map((tool) => (
            <ToolLabel key={tool} harness={tool} hideName />
          ))}
        </span>
      </TableCell>
      <TableCell className="px-3 py-2 text-right tabular-nums">
        <span className="inline-flex items-center justify-end gap-2">
          {gaps > 0 ? <Badge variant="warning">{formatCount(gaps)} gaps</Badge> : null}
          {formatAttributedCost(pr.priced_cost_usd_10000, pr.priced_request_count)}
        </span>
      </TableCell>
      <TableCell className="px-3 py-2 text-right tabular-nums">
        {formatHours(toMs(row.leadHours))}
      </TableCell>
      <TableCell className="text-muted-foreground px-3 py-2 text-right">{EMPTY_VALUE}</TableCell>
      <TableCell className="bg-card shadow-sticky-edge-end group-hover:bg-surface-muted group-data-[state=selected]:bg-muted sticky right-0 z-10 w-10 px-2 py-2">
        <Button
          variant="ghost"
          size="icon-sm"
          aria-expanded={open}
          aria-label={open ? `Collapse #${pr.number}` : `Expand #${pr.number}`}
          onClick={(event) => {
            event.stopPropagation()
            onToggle()
          }}
        >
          <AppIcon icon={open ? ArrowDown01Icon : ArrowRight01Icon} size={16} aria-hidden />
        </Button>
      </TableCell>
    </TableRow>
  )
}

/** One bar per attributed commit, height by attested lines, AI stacked over human. */
function CommitStrip({
  commits,
  attributed,
  total,
}: {
  commits: CommitView[]
  attributed: number
  total: number
}) {
  const max = Math.max(1, ...commits.map((commit) => commit.ai_lines + commit.human_lines))
  return (
    <div className="flex items-end gap-2">
      <div
        className="flex h-7 items-end gap-0.5"
        role="img"
        aria-label={`${attributed} of ${total} commits have notes`}
      >
        {commits.slice(0, 16).map((commit) => {
          const lines = commit.ai_lines + commit.human_lines
          return (
            <span
              key={commit.commit_sha}
              title={`${shortSha(commit.commit_sha)}: ${commit.ai_lines} AI, ${commit.human_lines} human`}
              className="flex w-1.5 flex-col justify-end overflow-hidden rounded-sm"
              style={{ height: Math.max(3, (lines / max) * 28) }}
            >
              <span
                className="bg-(--ai)"
                style={
                  {
                    '--ai': AI_TONE,
                    height: `${(commit.ai_lines / Math.max(1, lines)) * 100}%`,
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
      <span className="text-muted-foreground text-xs tabular-nums">
        {commits.length}/{total}
      </span>
    </div>
  )
}

/** First commit to merge on one axis, with the attributed commits and their requests. */
function PullRequestDetail({ row }: { row: PullRequestRow }) {
  const { pr } = row
  const end = new Date(pr.merged_at).getTime()
  const start = pr.first_commit_at ? new Date(pr.first_commit_at).getTime() : end
  const span = Math.max(1, end - start)
  const position = (at: string) =>
    `${Math.min(100, Math.max(0, ((new Date(at).getTime() - start) / span) * 100))}%`
  return (
    <div className="flex flex-col gap-4 p-4">
      <div className="flex flex-col gap-2">
        <div className="text-muted-foreground flex justify-between gap-3 text-xs">
          <span className="whitespace-nowrap">
            First commit · {pr.first_commit_at ? formatTimestamp(pr.first_commit_at) : EMPTY_VALUE}
          </span>
          <span className="whitespace-nowrap">Merged · {formatTimestamp(pr.merged_at)}</span>
        </div>
        <div className="relative h-8">
          <div className="bg-border absolute inset-x-0 top-1/2 h-px" />
          {pr.commits.map((commit) => (
            <span
              key={commit.commit_sha}
              className="border-card absolute top-1/2 size-3 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 bg-(--dot)"
              style={
                {
                  left: position(commit.committed_at),
                  '--dot': commit.ai_lines > commit.human_lines ? AI_TONE : HUMAN_TONE,
                } as CSSProperties
              }
              title={`${shortSha(commit.commit_sha)} · ${formatTimestamp(commit.committed_at)}`}
            />
          ))}
          <span
            className="bg-foreground absolute top-1/2 right-0 h-4 w-px -translate-y-1/2"
            title="Merged"
          />
        </div>
        <div className="text-muted-foreground flex flex-wrap items-center gap-4 text-xs">
          <span>Lead time: {formatHours(toMs(row.leadHours))}</span>
          <span className="inline-flex items-center gap-1.5">
            Review wait: {EMPTY_VALUE}
            <ReadinessBadge
              level="new"
              note="Needs the pull request's opened and first review times."
            />
          </span>
          <span>
            {formatCount(pr.attributed_commit_count)} of {formatCount(pr.commit_count)} commits have
            notes
            {pr.commits.length === 0 && pr.attributed_commit_count > 0
              ? '; commit details are left out of a report this large, so choose a shorter window or a repository'
              : pr.commits.length < pr.attributed_commit_count
                ? `; the first ${formatCount(pr.commits.length)} are shown`
                : ''}
          </span>
        </div>
      </div>
      {pr.commits.length > 0 ? (
        <div className="border-border overflow-x-auto rounded-md border">
          <Table aria-label={`Commits in #${pr.number}`}>
            <AdminTableHeader
              columns={[
                'Commit',
                'Committed',
                'Harness',
                'Models',
                { label: 'AI lines', align: 'right' },
                { label: 'Human lines', align: 'right' },
                { label: 'Requests', align: 'right' },
                { label: 'Cost', align: 'right' },
              ]}
            />
            <TableBody>
              {pr.commits.map((commit) => (
                <TableRow key={commit.commit_sha}>
                  <TableCell className="px-3 py-1.5 font-mono text-xs">
                    {shortSha(commit.commit_sha)}
                  </TableCell>
                  <TableCell className="px-3 py-1.5 whitespace-nowrap">
                    {formatTimestamp(commit.committed_at)}
                  </TableCell>
                  <TableCell className="px-3 py-1.5">
                    <span className="flex items-center gap-2">
                      {commit.agent_tools.length === 0 ? EMPTY_VALUE : null}
                      {commit.agent_tools.map((tool) => (
                        <ToolLabel key={tool} harness={tool} />
                      ))}
                    </span>
                  </TableCell>
                  <TableCell
                    className="max-w-56 truncate px-3 py-1.5 font-mono text-xs"
                    title={commit.models.join(', ')}
                  >
                    {commit.models.join(', ') || EMPTY_VALUE}
                  </TableCell>
                  <TableCell className="px-3 py-1.5 text-right tabular-nums">
                    {formatCount(commit.ai_lines)}
                  </TableCell>
                  <TableCell className="px-3 py-1.5 text-right tabular-nums">
                    {formatCount(commit.human_lines)}
                  </TableCell>
                  <TableCell className="px-3 py-1.5 text-right tabular-nums">
                    {formatCount(commit.request_count)}
                  </TableCell>
                  <TableCell className="px-3 py-1.5 text-right tabular-nums">
                    {formatAttributedCost(
                      commit.priced_cost_usd_10000,
                      commit.priced_request_count,
                    )}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      ) : null}
    </div>
  )
}
