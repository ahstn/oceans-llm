// Delivery: DORA measures through an AI lens. Oceans sees merges, not deploys, so deployment
// frequency and change fail rate are labelled proxies, and recovery and rework wait on deploy and
// incident data. Merged pull requests are then split by how much of each AI wrote.

import { Histogram, LeadTimeScatter, LeadTimeTrendChart } from '@/components/attribution/charts'
import {
  DECIMAL_1,
  formatHours,
  formatPercent,
  InlineMeter,
  Insight,
  InsightCard,
  ReadinessBadge,
  type Readiness,
} from '@/components/attribution/parts'
import { AdminTableHeader } from '@/components/shared/admin-table-header'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardDescription, CardHeader } from '@/components/ui/card'
import { Table, TableBody, TableCell, TableRow } from '@/components/ui/table'
import { EMPTY_VALUE, formatCount, formatUsd10000 } from '@/lib/format'
import type { AttributionReportView } from '@/types/api'

import {
  bandSummaries,
  median,
  pullRequestRows,
  ratio,
  sizeHistogram,
  weeklyLeadTimes,
  type BandSummary,
} from '../insights'

type Band = 'Elite' | 'High' | 'Medium' | 'Low'
const HOUR_MS = 3_600_000

// A tab is one linear layout read top to bottom; splitting it would scatter the sections.
// oxlint-disable-next-line eslint/max-lines-per-function
export default function DeliveryTab({ report }: { report: AttributionReportView }) {
  const rows = pullRequestRows(report.pull_requests)
  const leadHours = rows.flatMap((row) => (row.leadHours == null ? [] : [row.leadHours]))
  const medianLead = median(leadHours)
  const mergesPerDay = rows.length / report.window_days
  const failRate = ratio(rows.filter((row) => row.pr.is_revert).length, rows.length)
  const bands = bandSummaries(rows)
  const weeks = Math.max(1, Math.floor(report.window_days / 7))

  if (rows.length === 0) {
    return (
      <InsightCard title="Delivery" description="No merged pull requests in this window.">
        <p className="text-muted-foreground py-6 text-center text-sm">
          Delivery measures start once <code className="font-mono">oceans hook prs</code> reports
          merged pull requests.
        </p>
      </InsightCard>
    )
  }

  return (
    <div className="flex flex-col gap-6">
      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-5">
        <DoraTile
          label="Deployment frequency"
          value={`${DECIMAL_1.format(mergesPerDay)} / day`}
          band={
            mergesPerDay >= 1
              ? 'Elite'
              : mergesPerDay >= 1 / 7
                ? 'High'
                : mergesPerDay >= 1 / 30
                  ? 'Medium'
                  : 'Low'
          }
          note="Merged pull requests per day"
          readiness="proxy"
          readinessNote="Merges stand in for deploys until deploy events reach Oceans."
        />
        <DoraTile
          label="Lead time for changes"
          value={formatHours(medianLead == null ? null : medianLead * HOUR_MS)}
          band={
            medianLead == null
              ? undefined
              : medianLead < 24
                ? 'Elite'
                : medianLead < 24 * 7
                  ? 'High'
                  : medianLead < 24 * 30
                    ? 'Medium'
                    : 'Low'
          }
          note="First commit to merge, median"
        />
        <DoraTile
          label="Change fail rate"
          value={formatPercent(failRate, 1)}
          band={
            failRate == null
              ? undefined
              : failRate <= 0.05
                ? 'Elite'
                : failRate <= 0.1
                  ? 'High'
                  : failRate <= 0.15
                    ? 'Medium'
                    : 'Low'
          }
          note="Merged pull requests that are reverts"
          readiness="proxy"
          readinessNote="Matched by title (Revert or revert:). Incidents linked to changes would make it real."
        />
        <DoraTile
          label="Recovery time"
          value={EMPTY_VALUE}
          note="Failed change to restored service"
          readiness="new"
          readinessNote="Needs incident or failed-deploy events."
        />
        <DoraTile
          label="Deployment rework rate"
          value={EMPTY_VALUE}
          note="Unplanned deploys after incidents"
          readiness="new"
          readinessNote="Needs deploy events tagged as unplanned."
        />
      </div>

      <div className="grid gap-6 xl:grid-cols-2">
        <InsightCard
          title="Lead time trend"
          description="First commit to merge, by week of merge. The 85th percentile shows the slow tail that a median hides."
        >
          <LeadTimeTrendChart rows={weeklyLeadTimes(rows, report.window_end, weeks)} />
        </InsightCard>
        <InsightCard
          title="Lead time against AI share"
          description="One dot per merged pull request, sized by added lines. A cloud rising to the right means AI-heavy work waits longer."
        >
          <LeadTimeScatter
            medianHours={medianLead}
            points={rows.flatMap((row) =>
              row.leadHours == null
                ? []
                : [
                    {
                      aiShare: row.aiShare,
                      leadHours: Math.max(1, row.leadHours),
                      lines: row.added,
                      label: `#${row.pr.number} ${row.pr.title}`,
                      cost:
                        row.pr.priced_request_count > 0
                          ? formatUsd10000(row.pr.priced_cost_usd_10000)
                          : EMPTY_VALUE,
                    },
                  ],
            )}
          />
        </InsightCard>
      </div>

      <InsightCard
        title="Delivery by AI share"
        description="Merged pull requests grouped by how much of their attested code AI wrote. Bands beat a yes or no flag: a pull request with one AI line is not agent work."
      >
        <div className="flex flex-col gap-4">
          <div className="border-border overflow-x-auto rounded-md border">
            <Table aria-label="Delivery by AI share" className="min-w-[56rem]">
              <AdminTableHeader
                columns={[
                  'AI share',
                  { label: 'Merged PRs', align: 'right' },
                  { label: 'Median size', align: 'right' },
                  'Lead time, median',
                  { label: 'Review wait', align: 'right' },
                  { label: 'Reverts', align: 'right' },
                  { label: 'Cost per PR', align: 'right' },
                ]}
              />
              <TableBody>
                {bands.map((band) => (
                  <BandRow key={band.key} band={band} total={rows.length} bands={bands} />
                ))}
              </TableBody>
            </Table>
          </div>
          <Insight>{bandComparison(bands)}</Insight>
        </div>
      </InsightCard>

      <InsightCard
        title="Batch size"
        description="Attested lines added per merged pull request. DORA's AI capabilities model names small batches as one of the practices that make AI pay off."
      >
        <Histogram
          bins={sizeHistogram(rows)}
          medianLabel={`Median ${formatCount(median(rows.map((row) => row.added)) ?? 0)}`}
        />
      </InsightCard>
    </div>
  )
}

function BandRow({
  band,
  total,
  bands,
}: {
  band: BandSummary
  total: number
  bands: BandSummary[]
}) {
  const maxLead = Math.max(1, ...bands.map((entry) => entry.medianLeadHours ?? 0))
  return (
    <TableRow>
      <TableCell className="px-3 py-2 font-medium">{band.label}</TableCell>
      <TableCell className="px-3 py-2 text-right tabular-nums">
        {formatCount(band.count)}
        <span className="text-muted-foreground ml-1 text-xs">
          {formatPercent(ratio(band.count, total))}
        </span>
      </TableCell>
      <TableCell className="px-3 py-2 text-right tabular-nums">
        {band.medianSize == null ? EMPTY_VALUE : `${formatCount(band.medianSize)} lines`}
      </TableCell>
      <TableCell className="px-3 py-2">
        <InlineMeter
          value={band.medianLeadHours == null ? null : band.medianLeadHours / maxLead}
          label={formatHours(band.medianLeadHours == null ? null : band.medianLeadHours * HOUR_MS)}
          className="min-w-44"
        />
      </TableCell>
      <TableCell className="px-3 py-2 text-right">
        <ReadinessBadge level="new" note="Needs pull request opened and review times." />
      </TableCell>
      <TableCell className="px-3 py-2 text-right tabular-nums">
        {formatPercent(band.revertRate, 1)}
      </TableCell>
      <TableCell className="px-3 py-2 text-right tabular-nums">
        {band.medianCost == null ? EMPTY_VALUE : formatUsd10000(band.medianCost)}
      </TableCell>
    </TableRow>
  )
}

/** 90%+ AI against 1–49% AI: the light band is the baseline, as few PRs have no AI at all. */
function bandComparison(bands: BandSummary[]) {
  const light = bands.find((band) => band.key === 'light')
  const agentic = bands.find((band) => band.key === 'agentic')
  if (
    !light?.medianSize ||
    !agentic?.medianSize ||
    !light.medianLeadHours ||
    !agentic.medianLeadHours
  ) {
    return 'Too few merged pull requests in the 1–49% and 90%+ bands to compare them yet.'
  }
  return `90%+ AI pull requests are ${DECIMAL_1.format(agentic.medianSize / light.medianSize)}× the size of 1–49% AI ones and take ${DECIMAL_1.format(agentic.medianLeadHours / light.medianLeadHours)}× as long from first commit to merge. LinearB and Faros see AI move the bottleneck from writing to review.`
}

const BANDS: Band[] = ['Low', 'Medium', 'High', 'Elite']

function DoraTile({
  label,
  value,
  band,
  note,
  readiness,
  readinessNote,
}: {
  label: string
  value: string
  band?: Band
  note: string
  readiness?: Readiness
  readinessNote?: string
}) {
  const index = band ? BANDS.indexOf(band) : -1
  return (
    <Card size="sm" className="min-w-0">
      <CardHeader>
        <CardDescription className="flex flex-col items-start gap-1.5">
          <span>{label}</span>
          {readiness ? <ReadinessBadge level={readiness} note={readinessNote} /> : null}
        </CardDescription>
        <p className="text-2xl font-medium tracking-tight tabular-nums">{value}</p>
      </CardHeader>
      <CardContent className="flex flex-col gap-2">
        <div className="flex items-center gap-2">
          <div className="flex flex-1 gap-0.5" aria-hidden>
            {BANDS.map((entry, position) => (
              <span
                key={entry}
                className={
                  position <= index
                    ? 'bg-chart-hero h-1.5 flex-1 rounded-full'
                    : 'bg-muted h-1.5 flex-1 rounded-full'
                }
              />
            ))}
          </div>
          {band ? <Badge variant="outline">{band}</Badge> : null}
        </div>
        <span className="text-muted-foreground text-xs">{note}</span>
      </CardContent>
    </Card>
  )
}
