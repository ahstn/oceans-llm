// Concept 3, Delivery: DORA through an AI lens. The four keys come first, labelled honestly as
// proxies where Oceans sees merges rather than deploys, then the same flow split by how much of each
// PR AI wrote, which is where AI's effect on review queues and stability shows up.

import { useMemo } from 'react'

import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardDescription, CardHeader } from '@/components/ui/card'
import { Table, TableBody, TableCell, TableRow } from '@/components/ui/table'
import { AdminTableHeader } from '@/components/shared/admin-table-header'
import { formatCount, formatUsd10000 } from '@/lib/format'

import { Histogram, LeadTimeScatter, LeadTimeTrendChart } from './charts'
import { getMockDataset, HOUR_MS } from './fixtures'
import {
  AI_BANDS,
  aiBand,
  median,
  percentile,
  ratio,
  rollupPullRequest,
  sliceWindow,
  type ConceptWindow,
  type PullRequestRollup,
} from './metrics'
import {
  ConceptCard,
  DECIMAL_1,
  formatHours,
  formatPercent,
  InlineMeter,
  Insight,
  ReadinessBadge,
  type Readiness,
} from './parts'

type Band = 'Elite' | 'High' | 'Medium' | 'Low'

// A concept page is one linear layout read top to bottom; splitting it would scatter the mockup.
// oxlint-disable-next-line eslint/max-lines-per-function
export default function DeliveryConcept({ days }: { days: ConceptWindow }) {
  const model = useMemo(() => {
    const dataset = getMockDataset()
    const slice = sliceWindow(dataset, days)
    const rollups = slice.merged.map((pr) => rollupPullRequest(slice, pr))
    const leadHours = rollups.flatMap((row) => (row.leadTimeMs ? [row.leadTimeMs / HOUR_MS] : []))
    const weeks = Array.from({ length: 13 }, (_, index) => {
      const week = sliceWindow(dataset, 7, 12 - index)
      const hours = week.merged.map((pr) => ((pr.mergedAt ?? 0) - pr.firstCommitAt) / HOUR_MS)
      return {
        day: new Date(week.from + 3.5 * 86_400_000).toISOString().slice(0, 10),
        p50: median(hours),
        p85: percentile(hours, 0.85),
      }
    })
    return { slice, rollups, leadHours, weeks }
  }, [days])

  const { rollups, leadHours, slice } = model
  const deploysPerDay = rollups.length / days
  const medianLead = median(leadHours)
  const changeFail = ratio(rollups.filter((row) => row.pr.reverted).length, rollups.length)
  const bands = AI_BANDS.map((band) => {
    const rows = rollups.filter((row) => aiBand(row.aiShare) === band.key)
    return { band, rows, summary: summarise(rows) }
  })
  const sizeBins = binSizes(rollups)

  return (
    <div className="flex flex-col gap-6">
      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-5">
        <DoraTile
          label="Deployment frequency"
          value={`${DECIMAL_1.format(deploysPerDay)} / day`}
          band={
            deploysPerDay >= 1
              ? 'Elite'
              : deploysPerDay >= 1 / 7
                ? 'High'
                : deploysPerDay >= 1 / 30
                  ? 'Medium'
                  : 'Low'
          }
          note="Merges to the default branch"
          readiness="proxy"
          readinessNote="Stands in for deploys. Deploy events would make it real."
        />
        <DoraTile
          label="Lead time for changes"
          value={formatHours(medianLead == null ? null : medianLead * HOUR_MS)}
          band={
            medianLead == null
              ? 'Low'
              : medianLead < 24
                ? 'Elite'
                : medianLead < 24 * 7
                  ? 'High'
                  : medianLead < 24 * 30
                    ? 'Medium'
                    : 'Low'
          }
          note="First commit to merge, median"
          readiness="derived"
          readinessNote="Commit times and merge times are both stored."
        />
        <DoraTile
          label="Change fail rate"
          value={formatPercent(changeFail, 1)}
          band={
            changeFail == null
              ? 'Low'
              : changeFail <= 0.05
                ? 'Elite'
                : changeFail <= 0.1
                  ? 'High'
                  : changeFail <= 0.15
                    ? 'Medium'
                    : 'Low'
          }
          note="Merged PRs later reverted"
          readiness="proxy"
          readinessNote="Detect revert PRs by title or by `This reverts commit`; incidents would make it real."
        />
        <DoraTile
          label="Recovery time"
          value="2.4 h"
          band="High"
          note="Failed change to fix merged"
          readiness="new"
          readinessNote="Needs incident or failed-deploy events."
        />
        <DoraTile
          label="Deployment rework rate"
          value="7%"
          band="High"
          note="Unplanned deploys after incidents"
          readiness="new"
          readinessNote="DORA's fifth metric since 2024. Needs deploy events tagged as unplanned."
        />
      </div>

      <div className="grid gap-6 xl:grid-cols-2">
        <ConceptCard
          title="Lead time trend"
          description="First commit to merge, by week. The 85th percentile shows the slow tail that a median hides."
          readiness="derived"
        >
          <LeadTimeTrendChart rows={model.weeks} />
        </ConceptCard>
        <ConceptCard
          title="Lead time against AI share"
          description="One dot per merged PR, sized by added lines. A cloud rising to the right means AI-heavy work waits longer."
          readiness="derived"
        >
          <LeadTimeScatter
            medianHours={medianLead}
            points={rollups.flatMap((row) =>
              row.leadTimeMs
                ? [
                    {
                      aiShare: row.aiShare,
                      leadHours: Math.max(1, row.leadTimeMs / HOUR_MS),
                      lines: row.addedLines,
                      label: `#${row.pr.number} ${row.pr.title}`,
                      cost: formatUsd10000(row.costUsd10000),
                    },
                  ]
                : [],
            )}
          />
        </ConceptCard>
      </div>

      <ConceptCard
        title="Delivery by AI share"
        description="Merged PRs grouped by how much of their added code AI wrote. Bands beat a yes or no flag: a PR with one AI line is not agent work."
        readiness="derived"
        readinessNote="Review wait needs PR opened_at from `oceans hook prs`; reverts are a proxy."
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
                  { label: 'Reverted', align: 'right' },
                  { label: 'Cost per PR', align: 'right' },
                ]}
              />
              <TableBody>
                {bands.map(({ band, rows, summary }) => (
                  <TableRow key={band.key}>
                    <TableCell className="px-3 py-2 font-medium">{band.label}</TableCell>
                    <TableCell className="px-3 py-2 text-right tabular-nums">
                      {formatCount(rows.length)}
                      <span className="text-muted-foreground ml-1 text-xs">
                        {formatPercent(ratio(rows.length, rollups.length))}
                      </span>
                    </TableCell>
                    <TableCell className="px-3 py-2 text-right tabular-nums">
                      {summary.size == null ? '—' : `${formatCount(summary.size)} lines`}
                    </TableCell>
                    <TableCell className="px-3 py-2">
                      <InlineMeter
                        value={
                          summary.leadHours == null ? null : summary.leadHours / bandsMaxLead(bands)
                        }
                        label={formatHours(
                          summary.leadHours == null ? null : summary.leadHours * HOUR_MS,
                        )}
                        className="min-w-44"
                      />
                    </TableCell>
                    <TableCell className="px-3 py-2 text-right tabular-nums">
                      <span className="inline-flex items-center gap-2">
                        <ReadinessBadge level="new" />
                        {formatHours(
                          summary.reviewHours == null ? null : summary.reviewHours * HOUR_MS,
                        )}
                      </span>
                    </TableCell>
                    <TableCell className="px-3 py-2 text-right tabular-nums">
                      {formatPercent(summary.revertRate, 1)}
                    </TableCell>
                    <TableCell className="px-3 py-2 text-right tabular-nums">
                      {summary.cost ? formatUsd10000(summary.cost) : '—'}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
          <Insight>
            {bandComparison(bands)} LinearB and Faros see the same pattern: with agents, the
            bottleneck moves from writing code to reviewing it.
          </Insight>
        </div>
      </ConceptCard>

      <ConceptCard
        title="Batch size"
        description="Added lines per merged PR. DORA's AI capabilities model names small batches as one of the seven practices that make AI pay off."
        readiness="derived"
      >
        <Histogram
          bins={sizeBins}
          medianLabel={`Median ${formatCount(median(rollups.map((row) => row.addedLines)) ?? 0)}`}
        />
      </ConceptCard>
      <p className="text-muted-foreground text-xs">
        Window: {formatCount(slice.merged.length)} merged PRs. DORA bands follow the 2024 report
        thresholds.
      </p>
    </div>
  )
}

function summarise(rows: PullRequestRollup[]) {
  return {
    size: median(rows.map((row) => row.addedLines)),
    leadHours: median(rows.flatMap((row) => (row.leadTimeMs ? [row.leadTimeMs / HOUR_MS] : []))),
    reviewHours: median(
      rows.flatMap((row) => (row.reviewTimeMs ? [row.reviewTimeMs / HOUR_MS] : [])),
    ),
    revertRate: ratio(rows.filter((row) => row.pr.reverted).length, rows.length),
    cost: median(rows.map((row) => row.costUsd10000)),
  }
}

function bandsMaxLead(bands: { summary: ReturnType<typeof summarise> }[]) {
  return Math.max(1, ...bands.map((entry) => entry.summary.leadHours ?? 0))
}

/** 90%+ AI against 1–49% AI: the light band is the baseline, as few PRs have no AI at all. */
function bandComparison(
  bands: { band: (typeof AI_BANDS)[number]; summary: ReturnType<typeof summarise> }[],
) {
  const light = bands.find((entry) => entry.band.key === 'light')?.summary
  const agentic = bands.find((entry) => entry.band.key === 'agentic')?.summary
  if (!light?.size || !agentic?.size || !light.leadHours || !agentic.leadHours) {
    return 'Too few PRs in a band to compare.'
  }
  return `90%+ AI PRs are ${DECIMAL_1.format(agentic.size / light.size)}× the size of 1–49% AI ones and take ${DECIMAL_1.format(agentic.leadHours / light.leadHours)}× as long from first commit to merge.`
}

const SIZE_EDGES = [0, 25, 50, 100, 200, 400, 800, 1600, Infinity]

function binSizes(rows: PullRequestRollup[]) {
  const med = median(rows.map((row) => row.addedLines)) ?? 0
  return SIZE_EDGES.slice(0, -1).map((low, index) => {
    const high = SIZE_EDGES[index + 1]
    return {
      label: high === Infinity ? `${low}+` : `${low}–${high}`,
      count: rows.filter((row) => row.addedLines >= low && row.addedLines < high).length,
      median: med >= low && med < high,
    }
  })
}

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
  band: Band
  note: string
  readiness: Readiness
  readinessNote?: string
}) {
  return (
    <Card size="sm" className="min-w-0">
      <CardHeader>
        <CardDescription className="flex flex-col items-start gap-1.5">
          <span>{label}</span>
          <ReadinessBadge level={readiness} note={readinessNote} />
        </CardDescription>
        <p className="text-2xl font-medium tracking-tight tabular-nums">{value}</p>
      </CardHeader>
      <CardContent className="flex flex-col gap-2">
        <DoraBandMeter band={band} />
        <span className="text-muted-foreground text-xs">{note}</span>
      </CardContent>
    </Card>
  )
}

const BANDS: Band[] = ['Low', 'Medium', 'High', 'Elite']

/** The four DORA performance bands with the current one filled. Neutral: a band is a position. */
function DoraBandMeter({ band }: { band: Band }) {
  const index = BANDS.indexOf(band)
  return (
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
      <Badge variant="outline">{band}</Badge>
    </div>
  )
}
