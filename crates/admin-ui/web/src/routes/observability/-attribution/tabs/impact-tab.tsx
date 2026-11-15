// Impact: what AI delivered and what it cost. Headline figures against the previous window, who
// wrote the code, spend against output, where agent sessions ended up, and the counterweights that
// keep the throughput figures honest. See docs/design-proposals/attribution.

import { useState } from 'react'

import { LinesMixChart, SpendOutputChart } from '@/components/attribution/charts'
import { FlowChart, type FlowLink, type FlowNode } from '@/components/attribution/flow-chart'
import {
  AI_TONE,
  AiShareBar,
  CounterweightPair,
  formatHours,
  formatPercent,
  InlineMeter,
  Insight,
  InsightCard,
  SHIPPED_TONE,
  Stat,
  TrendKpi,
  UNSHIPPED_TONE,
} from '@/components/attribution/parts'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { EMPTY_VALUE, formatCount, formatUsd10000 } from '@/lib/format'
import type { AttributionReportView } from '@/types/api'

import { bucketDays, chartBucketSize, delta, mergedAiShare, ratio, shippedShare } from '../insights'
import { UserActivityTable } from '../user-activity-table'

type Outcome = AttributionReportView['session_outcomes'][number]['outcome']
type Measure = 'cost' | 'sessions'

// A tab is one linear layout read top to bottom; splitting it would scatter the sections.
// oxlint-disable-next-line eslint/max-lines-per-function
export default function ImpactTab({
  report,
  onSelectUser,
}: {
  report: AttributionReportView
  onSelectUser?: (userId: string) => void
}) {
  const { summary, previous_summary: previous, totals, coverage } = report
  const buckets = bucketDays(report.daily, chartBucketSize(report.window_days))
  const linesPerDollar = ratio(totals.ai_lines, totals.priced_cost_usd_10000 / 10_000)
  const previousLinesPerDollar = ratio(previous.ai_lines, previous.priced_cost_usd_10000 / 10_000)
  const revertRate = ratio(summary.reverted_pull_request_count, summary.merged_pull_request_count)
  const previousRevertRate = ratio(
    previous.reverted_pull_request_count,
    previous.merged_pull_request_count,
  )

  return (
    <div className="flex flex-col gap-6">
      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        <TrendKpi
          label="AI share of merged code"
          value={formatPercent(mergedAiShare(summary))}
          delta={delta(mergedAiShare(summary), mergedAiShare(previous))}
          spark={buckets.map((bucket) => bucket.aiShare)}
          tone={AI_TONE}
          hint={`${formatCount(summary.merged_ai_lines)} AI lines merged`}
        />
        <TrendKpi
          label="Cost per merged PR"
          value={
            summary.median_pull_request_cost_usd_10000 == null
              ? EMPTY_VALUE
              : formatUsd10000(summary.median_pull_request_cost_usd_10000)
          }
          delta={delta(
            summary.median_pull_request_cost_usd_10000,
            previous.median_pull_request_cost_usd_10000,
          )}
          hint="Median, gateway-attributed"
        />
        <TrendKpi
          label="Merged PRs"
          value={formatCount(summary.merged_pull_request_count)}
          delta={delta(summary.merged_pull_request_count, previous.merged_pull_request_count)}
          spark={buckets.map((bucket) => bucket.mergedPrs)}
          hint={`${formatCount(summary.commit_count)} commits`}
        />
        <TrendKpi
          label="Spend that shipped"
          value={formatPercent(shippedShare(summary))}
          delta={delta(shippedShare(summary), shippedShare(previous))}
          spark={buckets.map((bucket) =>
            ratio(bucket.committedCost, bucket.committedCost + bucket.uncommittedCost),
          )}
          tone={SHIPPED_TONE}
          hint={`of ${formatUsd10000(summary.priced_cost_usd_10000)} priced spend`}
        />
      </div>

      <div className="grid gap-6 xl:grid-cols-2">
        <InsightCard
          title="Who wrote the code"
          description="Attested lines added per period. Unattested lines are unknown, not human, so they stay out of the share."
        >
          <div className="flex flex-col gap-4">
            <AiShareBar ai={totals.ai_lines} human={totals.human_lines} legend />
            <LinesMixChart rows={buckets} />
            <Insight>
              AI share of attested lines moved from{' '}
              {formatPercent(ratio(previous.ai_lines, previous.ai_lines + previous.human_lines))} to{' '}
              {formatPercent(ratio(totals.ai_lines, totals.ai_lines + totals.human_lines))}. Read it
              with the counterweights below: more AI lines only help if they last.
            </Insight>
          </div>
        </InsightCard>
        <InsightCard
          title="Spend against output"
          description="Priced gateway cost, split by whether a session reached a commit, against pull requests merged in the same period."
        >
          <div className="flex flex-col gap-4">
            <div className="grid grid-cols-3 gap-4">
              <Stat label="Priced spend" value={formatUsd10000(summary.priced_cost_usd_10000)} />
              <Stat label="Reached a commit" value={formatUsd10000(totals.priced_cost_usd_10000)} />
              <Stat
                label="No commit"
                value={formatUsd10000(totals.uncommitted_cost_usd_10000)}
                hint={formatPercent(
                  ratio(totals.uncommitted_cost_usd_10000, summary.priced_cost_usd_10000),
                )}
              />
            </div>
            <SpendOutputChart rows={buckets} />
          </div>
        </InsightCard>
      </div>

      <SessionOutcomesCard outcomes={report.session_outcomes} />

      <InsightCard
        title="Counterweights"
        description="Each throughput figure sits beside the measure that would expose gaming it. A team can raise the left column by shipping worse work; the right column catches it."
      >
        <div className="grid gap-3 lg:grid-cols-3">
          <CounterweightPair
            left={{
              label: 'AI share of merged code',
              value: formatPercent(mergedAiShare(summary)),
              delta: delta(mergedAiShare(summary), mergedAiShare(previous)),
            }}
            right={{
              label: 'AI lines unchanged after 30 days',
              value: EMPTY_VALUE,
              delta: null,
              readiness: 'new',
            }}
          />
          <CounterweightPair
            left={{
              label: 'Merged PRs',
              value: formatCount(summary.merged_pull_request_count),
              delta: delta(summary.merged_pull_request_count, previous.merged_pull_request_count),
            }}
            right={{
              label: 'Merged PRs that are reverts',
              value: formatPercent(revertRate, 1),
              delta: delta(revertRate, previousRevertRate),
              readiness: 'proxy',
            }}
          />
          <CounterweightPair
            left={{
              label: 'AI lines per dollar',
              value: linesPerDollar == null ? EMPTY_VALUE : formatCount(linesPerDollar),
              delta: delta(linesPerDollar, previousLinesPerDollar),
            }}
            right={{
              label: 'First commit to merge',
              value: formatHours(
                summary.median_lead_time_seconds == null
                  ? null
                  : summary.median_lead_time_seconds * 1000,
              ),
              delta: delta(summary.median_lead_time_seconds, previous.median_lead_time_seconds),
            }}
          />
        </div>
      </InsightCard>

      <InsightCard
        title="Data coverage"
        description="How much of the window the join could see. Low coverage makes every figure above an undercount."
      >
        <div className="grid gap-x-8 gap-y-3 md:grid-cols-3">
          <CoverageRow
            label="AI sessions joined to gateway traffic"
            value={ratio(totals.joined_session_count, totals.ai_session_count)}
          />
          <CoverageRow
            label="Commits whose author matched a user"
            value={ratio(
              totals.commit_count - totals.unmatched_author_commit_count,
              totals.commit_count,
            )}
          />
          <CoverageRow
            label="Requests with a price"
            value={ratio(coverage.priced_request_count, coverage.request_count)}
          />
        </div>
      </InsightCard>

      {report.users.length > 0 ? (
        <InsightCard
          title="User activity"
          description={
            onSelectUser
              ? "A commit hooked with a user's API key counts for that user. Other commits, and merged pull requests, count for their author's email. Select a user's name to filter the report to them."
              : "A commit hooked with a user's API key counts for that user. Other commits, and merged pull requests, count for their author's email."
          }
        >
          <UserActivityTable
            users={report.users}
            selectedUserId={report.user_id}
            onSelectUser={onSelectUser}
          />
        </InsightCard>
      ) : null}
    </div>
  )
}

const OUTCOME_LABELS: Record<Outcome, string> = {
  merged: 'Merged PR',
  default_branch: 'Default branch',
  unmerged: 'Not merged yet',
  no_commit: 'No commit',
}

/** Where agent sessions ended up, sized by spend or by sessions. */
function SessionOutcomesCard({
  outcomes,
}: {
  outcomes: AttributionReportView['session_outcomes']
}) {
  const [measure, setMeasure] = useState<Measure>('cost')
  const tally = (outcome: Outcome) => outcomes.find((entry) => entry.outcome === outcome)
  const sessionsOf = (list: Outcome[]) =>
    list.reduce((total, outcome) => total + (tally(outcome)?.session_count ?? 0), 0)
  const costOf = (list: Outcome[]) =>
    list.reduce((total, outcome) => total + (tally(outcome)?.priced_cost_usd_10000 ?? 0), 0)
  const value = (list: Outcome[]) => (measure === 'cost' ? costOf(list) : sessionsOf(list))
  const all: Outcome[] = ['merged', 'default_branch', 'unmerged', 'no_commit']
  const committed: Outcome[] = ['merged', 'default_branch', 'unmerged']
  const shipped: Outcome[] = ['merged', 'default_branch']
  const totalSessions = sessionsOf(all)
  const totalCost = costOf(all)
  const detail = (list: Outcome[]) =>
    `${formatCount(sessionsOf(list))} · ${formatPercent(ratio(sessionsOf(list), totalSessions))} · ${formatUsd10000(costOf(list))}`
  const node = (
    id: string,
    label: string,
    column: number,
    list: Outcome[],
    tone: string,
    leak = false,
  ): FlowNode => ({
    id,
    label,
    column,
    value: value(list),
    tone,
    detail: detail(list),
    leak,
  })
  const nodes = [
    node('all', 'All sessions', 0, all, 'var(--chart-3)'),
    node('committed', 'Committed', 1, committed, 'var(--chart-1)'),
    node('no_commit', OUTCOME_LABELS.no_commit, 1, ['no_commit'], UNSHIPPED_TONE, true),
    node('merged', OUTCOME_LABELS.merged, 2, ['merged'], SHIPPED_TONE),
    node('default_branch', OUTCOME_LABELS.default_branch, 2, ['default_branch'], SHIPPED_TONE),
    node('unmerged', OUTCOME_LABELS.unmerged, 2, ['unmerged'], 'var(--chart-other)', true),
  ].filter((entry) => entry.value > 0)
  const links: FlowLink[] = (
    [
      ['all', 'committed', committed],
      ['all', 'no_commit', ['no_commit']],
      ['committed', 'merged', ['merged']],
      ['committed', 'default_branch', ['default_branch']],
      ['committed', 'unmerged', ['unmerged']],
    ] as const
  )
    .map(([source, target, list]) => ({ source, target, value: value([...list]) }))
    .filter((link) => link.value > 0)
  const unshipped = costOf(['no_commit', 'unmerged'])

  return (
    <InsightCard
      title="Session outcomes"
      description="Every agent session in the window, followed to the furthest stage its commits reached. Open and closed pull requests are not reported yet, so unmerged work stays in one stage."
      action={
        <ToggleGroup
          type="single"
          variant="outline"
          size="sm"
          value={measure}
          onValueChange={(next) => next && setMeasure(next as Measure)}
          aria-label="Size flows by"
        >
          <ToggleGroupItem value="cost">Spend</ToggleGroupItem>
          <ToggleGroupItem value="sessions">Sessions</ToggleGroupItem>
        </ToggleGroup>
      }
    >
      {totalSessions === 0 ? (
        <p className="text-muted-foreground py-6 text-center text-sm">
          No agent sessions in this window.
        </p>
      ) : (
        <div className="flex flex-col gap-4">
          <FlowChart
            nodes={nodes}
            links={links}
            height={320}
            columns={[
              {
                title: 'Started',
                value: measure === 'cost' ? formatUsd10000(totalCost) : formatCount(totalSessions),
              },
              { title: 'Committed', value: formatPercent(ratio(value(committed), value(all))) },
              { title: 'Shipped', value: formatPercent(ratio(value(shipped), value(all))) },
            ]}
            ariaLabel="Agent sessions by outcome"
          />
          <Insight>
            {formatPercent(ratio(unshipped, totalCost))} of agent spend ({formatUsd10000(unshipped)}
            ) went to sessions that never committed or whose work has not merged. Git AI reports
            about 38% for typical teams.
          </Insight>
        </div>
      )}
    </InsightCard>
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
