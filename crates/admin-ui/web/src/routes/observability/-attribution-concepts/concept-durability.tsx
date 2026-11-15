// Concept 7, Durability: does AI code last? GitClear's churn and survival measures, split AI against
// human. Almost all of it needs new data, so the page doubles as the collection roadmap: each panel
// names what Oceans would have to record to make it real.

import { useMemo } from 'react'

import { AdminTableHeader } from '@/components/shared/admin-table-header'
import { Badge } from '@/components/ui/badge'
import { Table, TableBody, TableCell, TableRow } from '@/components/ui/table'
import { formatCount } from '@/lib/format'

import { SurvivalChart } from '@/components/attribution/charts'
import { getMockDataset } from './fixtures'
import {
  AI_BANDS,
  aiBand,
  ratio,
  rollupPullRequest,
  sliceWindow,
  sum,
  survivalCurve,
  type ConceptWindow,
} from './metrics'
import {
  InsightCard,
  InlineMeter,
  Insight,
  ReadinessBadge,
  TrendKpi,
} from '@/components/attribution/parts'
import { formatPercent, type Readiness } from '@/components/attribution/format'

const ROADMAP: {
  data: string
  unlocks: string
  source: string
  effort: 'Small' | 'Medium' | 'Large'
  readiness: Readiness
}[] = [
  {
    data: 'PR opened, first review and review count',
    unlocks: 'Review wait, merged without review, abandon rate',
    source:
      '`oceans hook prs` already runs `gh pr list`; add `createdAt`, `reviews` and closed-unmerged PRs to the same call.',
    effort: 'Small',
    readiness: 'new',
  },
  {
    data: 'User turns per session',
    unlocks: 'Steering and autonomy per harness and model',
    source: 'Count user-role messages in requests the gateway already logs. No new ingestion.',
    effort: 'Small',
    readiness: 'proxy',
  },
  {
    data: 'Revert and fix-up detection',
    unlocks: 'Change fail rate proxy by AI band',
    source: 'Match PR titles and `This reverts commit` bodies in the PR hook.',
    effort: 'Small',
    readiness: 'proxy',
  },
  {
    data: 'Deleted lines per commit',
    unlocks: 'Refactor rate, true PR size, net change',
    source:
      'The notes hook payload has the commit; add `git diff --numstat` totals, no file paths.',
    effort: 'Medium',
    readiness: 'new',
  },
  {
    data: 'Generated lines per session',
    unlocks: 'Generated : committed ratio, discarded work',
    source:
      'git-ai checkpoint stats (`git ai stats --json` `ai_additions` before commit) reported at session end.',
    effort: 'Medium',
    readiness: 'new',
  },
  {
    data: 'Line survival at 14, 30 and 90 days',
    unlocks: 'Churn, durability, cost per durable line',
    source:
      'A scheduled `git ai blame` pass per repository, reporting surviving AI and human line counts per merged PR.',
    effort: 'Large',
    readiness: 'new',
  },
  {
    data: 'Deploy and incident events',
    unlocks: 'Real DORA lead time, deploy frequency, failure and recovery',
    source: 'GitHub Deployments or a webhook from CD and the incident tool.',
    effort: 'Large',
    readiness: 'new',
  },
]

// A concept page is one linear layout read top to bottom; splitting it would scatter the mockup.
// oxlint-disable-next-line eslint/max-lines-per-function
export default function DurabilityConcept({ days }: { days: ConceptWindow }) {
  const model = useMemo(() => {
    const dataset = getMockDataset()
    const slice = sliceWindow(dataset, days)
    const rollups = slice.merged.map((pr) => rollupPullRequest(slice, pr))
    const aiLines = sum(rollups, (row) => row.aiLines)
    const durable = sum(rollups, (row) => row.pr.aiLinesDurable30)
    const reworked = sum(rollups, (row) => row.pr.aiLinesReworkedInReview)
    const weekly = Array.from({ length: 13 }, (_, index) => {
      const week = sliceWindow(dataset, 7, 12 - index)
      const rows = week.merged.map((pr) => rollupPullRequest(week, pr))
      return ratio(
        sum(rows, (row) => row.pr.aiLinesDurable30),
        sum(rows, (row) => row.aiLines),
      )
    })
    const bands = AI_BANDS.flatMap((band) => {
      if (band.key === 'none') return []
      const rows = rollups.filter((row) => aiBand(row.aiShare) === band.key)
      return {
        band,
        prs: rows.length,
        durable: ratio(
          sum(rows, (row) => row.pr.aiLinesDurable30),
          sum(rows, (row) => row.aiLines),
        ),
        rework: ratio(
          sum(rows, (row) => row.pr.aiLinesReworkedInReview),
          sum(rows, (row) => row.aiLines),
        ),
        reverted: ratio(rows.filter((row) => row.pr.reverted).length, rows.length),
      }
    })
    return {
      durable: ratio(durable, aiLines),
      rework: ratio(reworked, aiLines),
      revert: ratio(rollups.filter((row) => row.pr.reverted).length, rollups.length),
      curve: survivalCurve(slice),
      weekly,
      bands,
    }
  }, [days])

  const human30 = model.curve.find((point) => point.day === 30)?.human ?? null
  const ai14 = model.curve.find((point) => point.day === 14)?.ai ?? null

  return (
    <div className="flex flex-col gap-6">
      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        <TrendKpi
          label="AI lines unchanged at 30 days"
          value={formatPercent(model.durable)}
          spark={model.weekly}
          hint={`Human: ${formatPercent(human30)}`}
          readiness="new"
        />
        <TrendKpi
          label="AI lines churned within 14 days"
          value={formatPercent(ai14 == null ? null : 1 - ai14)}
          hint="GitClear's churn window"
          readiness="new"
        />
        <TrendKpi
          label="AI lines rewritten in review"
          value={formatPercent(model.rework)}
          hint="Changed between PR open and merge"
          readiness="new"
        />
        <TrendKpi
          label="Merged PRs reverted"
          value={formatPercent(model.revert, 1)}
          hint="Revert PRs matched by title"
          readiness="proxy"
        />
      </div>

      <div className="grid gap-6 xl:grid-cols-2">
        <InsightCard
          title="Line survival"
          description="Share of merged lines still unchanged N days after merge. The gap between the curves is the AI maintenance cost."
          readiness="new"
          readinessNote="Needs per-line history from scheduled git ai blame runs."
        >
          <SurvivalChart rows={model.curve} />
        </InsightCard>
        <InsightCard
          title="Quality by AI share"
          description="Merged PRs with any AI lines, by band. If durability falls as AI share rises, the heaviest agent work needs more review."
          readiness="new"
        >
          <div className="border-border overflow-x-auto rounded-md border">
            <Table aria-label="Quality by AI share" className="min-w-[28rem]">
              <AdminTableHeader
                columns={[
                  'AI share',
                  'Durable at 30 days',
                  'Reworked in review',
                  { label: 'Reverted', align: 'right' },
                ]}
              />
              <TableBody>
                {model.bands.map((row) => (
                  <TableRow key={row.band.key}>
                    <TableCell className="px-3 py-2">
                      <p className="font-medium">{row.band.label}</p>
                      <p className="text-muted-foreground text-xs">{formatCount(row.prs)} PRs</p>
                    </TableCell>
                    <TableCell className="px-3 py-2">
                      <InlineMeter value={row.durable} tone="var(--chart-2)" className="min-w-20" />
                    </TableCell>
                    <TableCell className="px-3 py-2">
                      <InlineMeter value={row.rework} tone="var(--chart-5)" className="min-w-20" />
                    </TableCell>
                    <TableCell className="px-3 py-2 text-right tabular-nums">
                      {formatPercent(row.reverted, 1)}
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
        description="What each addition unlocks across these concepts, smallest first. Line counts only: like today's notes, none of it stores file paths or line content."
        readiness="new"
      >
        <div className="flex flex-col gap-4">
          <div className="border-border overflow-x-auto rounded-md border">
            <Table aria-label="Data to collect" className="min-w-[64rem]">
              <AdminTableHeader columns={['Data', 'Unlocks', 'Source', 'Effort', 'Today']} />
              <TableBody>
                {ROADMAP.map((row) => (
                  <TableRow key={row.data}>
                    <TableCell className="max-w-56 px-3 py-2 font-medium whitespace-normal">
                      {row.data}
                    </TableCell>
                    <TableCell className="text-muted-foreground max-w-60 px-3 py-2 whitespace-normal">
                      {row.unlocks}
                    </TableCell>
                    <TableCell className="text-muted-foreground max-w-md min-w-72 px-3 py-2 text-xs whitespace-normal">
                      <InlineCode text={row.source} />
                    </TableCell>
                    <TableCell className="px-3 py-2">
                      <Badge variant="outline">{row.effort}</Badge>
                    </TableCell>
                    <TableCell className="px-3 py-2">
                      <ReadinessBadge level={row.readiness} />
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
          <Insight>
            The first three rows are a week of work and unlock review wait, autonomy and change fail
            rate. They cover most of the Delivery and Agents concepts.
          </Insight>
        </div>
      </InsightCard>
    </div>
  )
}

/** Renders `backticked` spans as code, for the roadmap's command names. */
function InlineCode({ text }: { text: string }) {
  // Each code span is keyed by where it starts in the text, which never changes.
  let offset = 0
  return text.split(/(`[^`]+`)/).map((part) => {
    const start = offset
    offset += part.length
    return part.startsWith('`') ? (
      <code key={start} className="text-foreground font-mono">
        {part.slice(1, -1)}
      </code>
    ) : (
      part
    )
  })
}
