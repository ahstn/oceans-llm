// Concept 1, Impact: the "what did AI deliver for what it cost" page. Every headline figure is paired
// with the metric that keeps it honest, so a rising AI share is read next to whether that code lasts.

import { useMemo } from 'react'

import { formatCount, formatUsd10000 } from '@/lib/format'

import { LinesMixChart, SpendOutputChart } from '@/components/attribution/charts'
import { getMockDataset } from './fixtures'
import {
  delta,
  headline,
  median,
  ratio,
  rollupPullRequest,
  series,
  sessionOutcomes,
  sliceWindow,
  sum,
  type ConceptWindow,
} from './metrics'
import {
  AI_TONE,
  AiShareBar,
  CounterweightPair,
  InsightCard,
  InlineMeter,
  Insight,
  SHIPPED_TONE,
  Stat,
  TrendKpi,
} from '@/components/attribution/parts'
import { formatHours, formatPercent } from '@/components/attribution/format'

// A concept page is one linear layout read top to bottom; splitting it would scatter the mockup.
// oxlint-disable-next-line eslint/max-lines-per-function
export default function ImpactConcept({ days }: { days: ConceptWindow }) {
  const model = useMemo(() => {
    const dataset = getMockDataset()
    const current = sliceWindow(dataset, days)
    const previous = sliceWindow(dataset, days, 1)
    const now = headline(current)
    const before = headline(previous)
    const bucket = days === 7 ? 1 : days === 30 ? 2 : 7
    const points = series(current, bucket)
    const weekly = series(sliceWindow(dataset, 90), 7)
    const outcomes = sessionOutcomes(dataset, current)
    const shippedCost =
      (outcomes.tally.get('merged')?.costUsd10000 ?? 0) +
      (outcomes.tally.get('direct')?.costUsd10000 ?? 0)
    const prevOutcomes = sessionOutcomes(dataset, previous)
    const prevShipped =
      (prevOutcomes.tally.get('merged')?.costUsd10000 ?? 0) +
      (prevOutcomes.tally.get('direct')?.costUsd10000 ?? 0)
    const rollups = current.merged.map((pr) => rollupPullRequest(current, pr))
    const durable = ratio(
      sum(rollups, (row) => row.pr.aiLinesDurable30),
      sum(rollups, (row) => row.aiLines),
    )
    const prevRollups = previous.merged.map((pr) => rollupPullRequest(previous, pr))
    const prevDurable = ratio(
      sum(prevRollups, (row) => row.pr.aiLinesDurable30),
      sum(prevRollups, (row) => row.aiLines),
    )
    const weeklyCostPerPr = Array.from({ length: 13 }, (_, index) => {
      const slice = sliceWindow(dataset, 7, 12 - index)
      return median(slice.merged.map((pr) => rollupPullRequest(slice, pr).costUsd10000 / 10_000))
    })
    return {
      now,
      before,
      points,
      weekly,
      shippedShare: ratio(shippedCost, now.costUsd10000),
      prevShippedShare: ratio(prevShipped, before.costUsd10000),
      durable,
      prevDurable,
      revertRate: ratio(rollups.filter((row) => row.pr.reverted).length, rollups.length),
      prevRevertRate: ratio(
        prevRollups.filter((row) => row.pr.reverted).length,
        prevRollups.length,
      ),
      weeklyCostPerPr,
      dataset,
      current,
    }
  }, [days])

  const { now, before } = model
  const sessionsJoined = ratio(now.joinedSessions, now.sessions)

  return (
    <div className="flex flex-col gap-6">
      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        <TrendKpi
          label="AI share of merged code"
          value={formatPercent(now.mergedAiShare)}
          delta={delta(now.mergedAiShare, before.mergedAiShare)}
          spark={model.weekly.map((point) => point.aiShare)}
          tone={AI_TONE}
          hint={`${formatCount(now.mergedAiLines)} AI lines merged`}
          readiness="derived"
          readinessNote="Sum the live per-PR AI and human lines over merged PRs."
        />
        <TrendKpi
          label="Cost per merged PR"
          value={
            now.medianCostPerMergedPr == null ? '—' : formatUsd10000(now.medianCostPerMergedPr)
          }
          delta={delta(now.medianCostPerMergedPr, before.medianCostPerMergedPr)}
          spark={model.weeklyCostPerPr}
          hint="Median, gateway-attributed"
          readiness="derived"
          readinessNote="Median of the live per-PR cost."
        />
        <TrendKpi
          label="Merged PRs"
          value={formatCount(now.mergedPrs)}
          delta={delta(now.mergedPrs, before.mergedPrs)}
          spark={model.weekly.map((point) => point.mergedPrs)}
          hint={`${formatCount(now.commits)} commits`}
          readiness="live"
        />
        <TrendKpi
          label="Spend that reached main"
          value={formatPercent(model.shippedShare)}
          delta={delta(model.shippedShare, model.prevShippedShare)}
          spark={model.weekly.map((point) =>
            ratio(point.committedCost, point.committedCost + point.uncommittedCost),
          )}
          tone={SHIPPED_TONE}
          hint={`of ${formatUsd10000(now.costUsd10000)} total`}
          readiness="derived"
          readinessNote="Session outcomes joined to merged PRs; uncommitted cost is already live."
        />
      </div>

      <div className="grid gap-6 xl:grid-cols-2">
        <InsightCard
          title="Who wrote the code"
          description="Attested lines added per period. Unattested lines are unknown, not human, so they stay out of the share."
          readiness="derived"
          readinessNote="Bucket live commit rows by committed_at."
        >
          <div className="flex flex-col gap-4">
            <AiShareBar ai={now.aiLines} human={now.humanLines} unknown={now.unknownLines} legend />
            <LinesMixChart rows={model.points} />
            <Insight>
              AI share moved from {formatPercent(before.aiShare)} to {formatPercent(now.aiShare)}.
              Read it with durability below: more AI lines is only good if they last.
            </Insight>
          </div>
        </InsightCard>
        <InsightCard
          title="Spend against output"
          description="Gateway cost split by whether a session reached a commit, against PRs merged in the same period."
          readiness="derived"
          readinessNote="Uncommitted cost is live as a total; per-period needs bucketing."
        >
          <div className="flex flex-col gap-4">
            <div className="grid grid-cols-3 gap-4">
              <Stat label="Total spend" value={formatUsd10000(now.costUsd10000)} />
              <Stat label="Reached a commit" value={formatUsd10000(now.committedCostUsd10000)} />
              <Stat
                label="No commit"
                value={formatUsd10000(now.uncommittedCostUsd10000)}
                hint={formatPercent(ratio(now.uncommittedCostUsd10000, now.costUsd10000))}
              />
            </div>
            <SpendOutputChart rows={model.points} />
          </div>
        </InsightCard>
      </div>

      <InsightCard
        title="Counterweights"
        description="Each throughput figure sits beside the measure that would expose gaming it. A team can raise the left column by shipping worse work; the right column catches it."
        readiness="proxy"
        readinessNote="Durability and reverts need new data; the rest is derivable."
      >
        <div className="grid gap-3 lg:grid-cols-3">
          <CounterweightPair
            left={{
              label: 'AI share of merged code',
              value: formatPercent(now.mergedAiShare),
              delta: delta(now.mergedAiShare, before.mergedAiShare),
              readiness: 'derived',
            }}
            right={{
              label: 'AI lines unchanged after 30 days',
              value: formatPercent(model.durable),
              delta: delta(model.durable, model.prevDurable),
              readiness: 'new',
            }}
          />
          <CounterweightPair
            left={{
              label: 'Merged PRs',
              value: formatCount(now.mergedPrs),
              delta: delta(now.mergedPrs, before.mergedPrs),
              readiness: 'live',
            }}
            right={{
              label: 'Merged PRs later reverted',
              value: formatPercent(model.revertRate, 1),
              delta: delta(model.revertRate, model.prevRevertRate),
              readiness: 'proxy',
            }}
          />
          <CounterweightPair
            left={{
              label: 'AI lines per dollar',
              value: now.aiLinesPerDollar == null ? '—' : formatCount(now.aiLinesPerDollar),
              delta: delta(now.aiLinesPerDollar, before.aiLinesPerDollar),
              readiness: 'derived',
            }}
            right={{
              label: 'First commit to merge',
              value: formatHours(now.medianLeadTimeMs),
              delta: delta(now.medianLeadTimeMs, before.medianLeadTimeMs),
              readiness: 'derived',
            }}
          />
        </div>
      </InsightCard>

      <InsightCard
        title="Data coverage"
        description="How much of the window the join could see. Low coverage makes every figure above an undercount."
        readiness="live"
      >
        <div className="grid gap-x-8 gap-y-3 md:grid-cols-3">
          <CoverageRow label="Sessions joined to gateway traffic" value={sessionsJoined} />
          <CoverageRow
            label="Committing authors using agents"
            value={ratio(now.activeAiUsers, now.committingUsers)}
          />
          <CoverageRow
            label="Added lines with an attestation"
            value={ratio(
              now.aiLines + now.humanLines,
              now.aiLines + now.humanLines + now.unknownLines,
            )}
          />
        </div>
      </InsightCard>
    </div>
  )
}

function CoverageRow({ label, value }: { label: string; value: number | null }) {
  return (
    <div className="flex flex-col gap-1.5">
      <span className="text-muted-foreground text-xs">{label}</span>
      <InlineMeter value={value} label={formatPercent(value)} />
    </div>
  )
}
