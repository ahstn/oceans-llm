// Repositories: where agents work well and where the harness needs investment. A quadrant plots
// AI share against autonomy (AI lines committed without human edits) until line survival lets it
// plot durability; the table gives each repository its trend, cost and lead time.

import { useMemo, useState } from 'react'

import {
  AI_TONE,
  InlineMeter,
  Insight,
  InsightCard,
  ReadinessBadge,
  Sparkline,
} from '@/components/attribution/parts'
import { formatHours, formatPercent } from '@/components/attribution/format'
import { Quadrant } from '@/components/attribution/quadrant'
import { AdminTableHead, AdminTableHeader } from '@/components/shared/admin-table-header'
import { Table, TableBody, TableCell, TableRow } from '@/components/ui/table'
import { EMPTY_VALUE, formatCount, formatUsd10000 } from '@/lib/format'
import type { AttributionReportView } from '@/types/api'

import { repositoryRows } from '../insights'

/** `github.com/acme/widgets` → `acme/widgets`. */
function shortRepository(name: string) {
  return name.replace(/^[^/]+\//, '')
}

// A tab is one linear layout read top to bottom; splitting it would scatter the sections.
// oxlint-disable-next-line eslint/max-lines-per-function
export default function RepositoriesTab({ report }: { report: AttributionReportView }) {
  const [focus, setFocus] = useState<string | null>(null)
  // Hovering a repository only changes focus, so the rows derive from the report alone.
  const rows = useMemo(
    () =>
      repositoryRows(report.changes, report.pull_requests, report.window_start, report.window_end),
    [report.changes, report.pull_requests, report.window_start, report.window_end],
  )
  const plotted = rows.filter(
    (row) => row.aiShare != null && row.autonomy != null && row.aiLines > 0,
  )
  const leastAutonomous = [...plotted].sort((a, b) => (a.autonomy ?? 0) - (b.autonomy ?? 0))[0]

  if (rows.length === 0) {
    return (
      <InsightCard title="Repositories" description="No attributed commits in this window.">
        <p className="text-muted-foreground py-6 text-center text-sm">
          Repositories appear once commits with Git AI notes reach Oceans.
        </p>
      </InsightCard>
    )
  }

  return (
    <div className="flex flex-col gap-6">
      <InsightCard
        title="Agent readiness"
        description="AI share across, autonomy (a commit-composition proxy) up, bubble size by spend. Dashed lines mark the average across repositories. Top right is where agents land work with little human change; bottom right is where people add much of their own to agent commits."
      >
        <div className="grid gap-8 lg:grid-cols-[minmax(0,24rem)_minmax(0,1fr)]">
          <div className="flex flex-col gap-3">
            <Quadrant
              ariaLabel="Repositories by AI share and autonomy"
              xLabel="AI share of attested lines"
              yLabel="Autonomy"
              labels={['Untapped', 'Leverage', 'Needs investment', 'Heavy steering']}
              focus={focus}
              onFocus={setFocus}
              points={plotted.map((row) => ({
                key: row.repository,
                label: shortRepository(row.repository).split('/').pop() ?? row.repository,
                x: row.aiShare ?? 0,
                y: row.autonomy ?? 0,
                size: Math.max(1, row.costUsd10000),
                weight: row.aiLines + row.humanLines,
                title: `${shortRepository(row.repository)}: ${formatPercent(row.aiShare)} AI, ${formatPercent(row.autonomy)} autonomous, ${formatUsd10000(row.costUsd10000)}`,
              }))}
            />
            <p className="text-muted-foreground flex flex-wrap items-center gap-1.5 text-xs">
              Plots 30-day durability instead of autonomy once line survival is collected.
              <ReadinessBadge
                level="new"
                note="Needs line survival after merge, from scheduled git ai blame runs."
              />
            </p>
          </div>
          <ul className="divide-border flex flex-col divide-y">
            {rows.map((row) => (
              <li
                key={row.repository}
                className={
                  focus && focus !== row.repository
                    ? 'grid gap-x-6 gap-y-2 py-3 opacity-40 transition-opacity duration-200 sm:grid-cols-[minmax(0,12rem)_minmax(0,1fr)_minmax(0,1fr)]'
                    : 'grid gap-x-6 gap-y-2 py-3 transition-opacity duration-200 sm:grid-cols-[minmax(0,12rem)_minmax(0,1fr)_minmax(0,1fr)]'
                }
                onMouseEnter={() => setFocus(row.repository)}
                onMouseLeave={() => setFocus(null)}
              >
                <span className="self-center truncate font-mono text-sm" title={row.repository}>
                  {shortRepository(row.repository)}
                </span>
                <div className="flex flex-col gap-1">
                  <span className="text-muted-foreground text-xs">Autonomy</span>
                  <InlineMeter value={row.autonomy} tone="var(--chart-3)" />
                </div>
                <div className="flex flex-col gap-1">
                  <span className="text-muted-foreground inline-flex items-center gap-1.5 text-xs">
                    Durable at 30 days <ReadinessBadge level="new" />
                  </span>
                  <InlineMeter value={null} label={EMPTY_VALUE} tone="var(--chart-2)" />
                </div>
              </li>
            ))}
          </ul>
        </div>
      </InsightCard>

      <InsightCard
        title="Repositories"
        description="Sorted by commits. Read across a row: a high AI share with low autonomy means agents need steering there, which usually means missing context."
      >
        <div className="flex flex-col gap-4">
          <div className="border-border overflow-x-auto rounded-md border">
            <Table aria-label="Repositories" className="min-w-[60rem]">
              <AdminTableHeader>
                <AdminTableHead>Repository</AdminTableHead>
                <AdminTableHead>AI share over the window</AdminTableHead>
                <AdminTableHead>Autonomy</AdminTableHead>
                <AdminTableHead className="text-right">
                  <span className="inline-flex items-center gap-1.5">
                    Generated : committed
                    <ReadinessBadge
                      level="new"
                      note="Needs generated lines per session from git-ai checkpoints."
                    />
                  </span>
                </AdminTableHead>
                <AdminTableHead className="text-right">Cost per PR</AdminTableHead>
                <AdminTableHead className="text-right">Lead time</AdminTableHead>
              </AdminTableHeader>
              <TableBody>
                {rows.map((row) => (
                  <TableRow
                    key={row.repository}
                    data-state={focus === row.repository ? 'selected' : undefined}
                    onMouseEnter={() => setFocus(row.repository)}
                    onMouseLeave={() => setFocus(null)}
                  >
                    <TableCell className="px-3 py-2">
                      <p className="font-mono text-sm">{shortRepository(row.repository)}</p>
                      <p className="text-muted-foreground text-xs">
                        {formatCount(row.commits)} commits · {formatCount(row.mergedPrs)} merged
                      </p>
                    </TableCell>
                    <TableCell className="w-48 px-3 py-2">
                      <div className="flex items-center gap-2">
                        <Sparkline
                          values={row.trend}
                          tone={AI_TONE}
                          height={24}
                          area={false}
                          className="w-24"
                        />
                        <span className="text-xs tabular-nums">{formatPercent(row.aiShare)}</span>
                      </div>
                    </TableCell>
                    <TableCell className="px-3 py-2">
                      <InlineMeter value={row.autonomy} tone="var(--chart-3)" />
                    </TableCell>
                    <TableCell className="text-muted-foreground px-3 py-2 text-right">
                      {EMPTY_VALUE}
                    </TableCell>
                    <TableCell className="px-3 py-2 text-right tabular-nums">
                      {row.medianPrCost == null ? EMPTY_VALUE : formatUsd10000(row.medianPrCost)}
                    </TableCell>
                    <TableCell className="px-3 py-2 text-right tabular-nums">
                      {formatHours(
                        row.medianLeadHours == null ? null : row.medianLeadHours * 3_600_000,
                      )}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
          <p className="text-muted-foreground text-xs">
            Autonomy is a proxy from commit composition: the share of AI lines that sit in commits
            where people added no more than 5% as many lines as the agent. Notes count lines per
            commit, so it can't tell whether people edited the agent's own lines.
          </p>
          {leastAutonomous && plotted.length > 1 ? (
            <Insight>
              <span className="font-mono">{shortRepository(leastAutonomous.repository)}</span> has
              the lowest autonomy at {formatPercent(leastAutonomous.autonomy)}: most of its AI lines
              land in commits where people also added a fair share of lines. An AGENTS.md, test
              fixtures or a skill there is likely to cut its spend more than a model change. Its
              median lead time is{' '}
              {formatHours(
                leastAutonomous.medianLeadHours == null
                  ? null
                  : leastAutonomous.medianLeadHours * 3_600_000,
              )}
              {leastAutonomous.medianPrCost == null
                ? '.'
                : ` and a merged pull request costs ${formatUsd10000(leastAutonomous.medianPrCost)} at the median.`}
            </Insight>
          ) : null}
        </div>
      </InsightCard>
    </div>
  )
}
