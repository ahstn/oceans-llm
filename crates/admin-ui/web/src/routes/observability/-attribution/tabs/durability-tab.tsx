// Durability: does AI-written code last? Line survival, churn and rework need data Oceans does not
// collect yet, so most of this tab carries "Needs new data" pills as a standing reminder. Reverts
// are matched by pull request title today. The collection plan lives in
// docs/internal/2026-11-15-attribution-data-gaps.md.

import { InsightCard, ReadinessBadge, TrendKpi } from '@/components/attribution/parts'
import { DATA_GAPS, DATA_GAPS_URL } from '@/components/attribution/data-gaps'
import { formatPercent } from '@/components/attribution/format'
import { AdminTableHeader } from '@/components/shared/admin-table-header'
import { Badge } from '@/components/ui/badge'
import { Table, TableBody, TableCell, TableRow } from '@/components/ui/table'
import { EMPTY_VALUE, formatCount } from '@/lib/format'
import type { AttributionReportView } from '@/types/api'

import { bandSummaries, pullRequestRows, ratio } from '../insights'

// A tab is one linear layout read top to bottom; splitting it would scatter the sections.
// oxlint-disable-next-line eslint/max-lines-per-function
export default function DurabilityTab({ report }: { report: AttributionReportView }) {
  const rows = pullRequestRows(report.pull_requests)
  const bands = bandSummaries(rows).filter((band) => band.key !== 'none')
  const revertRate = ratio(rows.filter((row) => row.pr.is_revert).length, rows.length)

  return (
    <div className="flex flex-col gap-6">
      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        <TrendKpi
          label="AI lines unchanged at 30 days"
          value={EMPTY_VALUE}
          hint="Against human lines over the same days"
          readiness="new"
          readinessNote="Needs line survival after merge."
        />
        <TrendKpi
          label="AI lines churned within 14 days"
          value={EMPTY_VALUE}
          hint="GitClear's churn window"
          readiness="new"
          readinessNote="Needs line survival after merge."
        />
        <TrendKpi
          label="AI lines rewritten in review"
          value={EMPTY_VALUE}
          hint="Changed between opening and merge"
          readiness="new"
          readinessNote="Needs pull request opened times and line history."
        />
        <TrendKpi
          label="Merged PRs that are reverts"
          value={formatPercent(revertRate, 1)}
          hint={`${formatCount(rows.filter((row) => row.pr.is_revert).length)} of ${formatCount(rows.length)}`}
          readiness="proxy"
          readinessNote="Found by git's revert line, GitHub's revert button or an exact inverse change; a revert title alone doesn't count."
        />
      </div>

      <div className="grid gap-6 xl:grid-cols-2">
        <InsightCard
          title="Line survival"
          description="Share of merged AI and human lines still unchanged N days after merge. The gap between the two curves is the maintenance cost of AI code."
          readiness="new"
          readinessNote="Needs line survival after merge, from scheduled git ai blame runs."
        >
          <div className="border-border text-muted-foreground flex min-h-56 flex-col items-center justify-center gap-2 rounded-md border border-dashed p-6 text-center text-sm">
            <p className="text-foreground font-medium">No survival data yet</p>
            <p className="max-w-sm">
              Oceans stores line counts per commit but not which lines survive later commits. A
              scheduled <code className="font-mono">git ai blame</code> pass would report surviving
              AI and human lines per merged pull request.
            </p>
          </div>
        </InsightCard>
        <InsightCard
          title="Quality by AI share"
          description="Merged pull requests with any AI lines, by band. If durability falls as AI share rises, the heaviest agent work needs more review."
        >
          <div className="border-border overflow-x-auto rounded-md border">
            <Table aria-label="Quality by AI share" className="min-w-[28rem]">
              <AdminTableHeader
                columns={[
                  'AI share',
                  'Durable at 30 days',
                  'Reworked in review',
                  { label: 'Reverts', align: 'right' },
                ]}
              />
              <TableBody>
                {bands.map((band) => (
                  <TableRow key={band.key}>
                    <TableCell className="px-3 py-2">
                      <p className="font-medium">{band.label}</p>
                      <p className="text-muted-foreground text-xs">{formatCount(band.count)} PRs</p>
                    </TableCell>
                    <TableCell className="px-3 py-2">
                      <ReadinessBadge level="new" />
                    </TableCell>
                    <TableCell className="px-3 py-2">
                      <ReadinessBadge level="new" />
                    </TableCell>
                    <TableCell className="px-3 py-2 text-right tabular-nums">
                      {formatPercent(band.revertRate, 1)}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        </InsightCard>
      </div>

      <InsightCard
        title="Data to collect"
        description={
          <>
            What each addition unlocks across the Attribution tabs, smallest first. Line counts
            only: like today's notes, none of it stores file paths or line content. The full plan is
            in{' '}
            <a
              href={DATA_GAPS_URL}
              target="_blank"
              rel="noopener noreferrer"
              className="text-primary underline underline-offset-4"
            >
              the attribution data gaps note
            </a>
            .
          </>
        }
      >
        <div className="border-border overflow-x-auto rounded-md border">
          <Table aria-label="Data to collect" className="min-w-[56rem]">
            <AdminTableHeader columns={['Data', 'Unlocks', 'Avenue', 'Effort', 'Today']} />
            <TableBody>
              {DATA_GAPS.map((row) => (
                <TableRow key={row.data}>
                  <TableCell className="max-w-56 px-3 py-2 font-medium whitespace-normal">
                    {row.data}
                  </TableCell>
                  <TableCell className="text-muted-foreground max-w-64 px-3 py-2 whitespace-normal">
                    {row.unlocks}
                  </TableCell>
                  <TableCell className="text-muted-foreground max-w-72 px-3 py-2 text-xs whitespace-normal">
                    {row.avenue}
                  </TableCell>
                  <TableCell className="px-3 py-2">
                    <Badge variant="outline">{row.effort}</Badge>
                  </TableCell>
                  <TableCell className="px-3 py-2">
                    <ReadinessBadge level={row.today} />
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      </InsightCard>
    </div>
  )
}
