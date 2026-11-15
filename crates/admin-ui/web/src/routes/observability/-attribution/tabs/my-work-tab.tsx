// My work: a personal view instead of a leaderboard. The report is limited to the signed-in user,
// and every comparison is against their own previous window, never against colleagues.

import { ContributionCalendar, PunchCard } from '@/components/attribution/heatmaps'
import {
  AI_TONE,
  AiShareBar,
  InlineMeter,
  InsightCard,
  ToolLabel,
  TrendKpi,
} from '@/components/attribution/parts'
import { formatHours, formatPercent } from '@/components/attribution/format'
import { EMPTY_VALUE, formatCount, formatTimestamp, formatUsd10000 } from '@/lib/format'
import type { AttributionReportView } from '@/types/api'

import { PullRequestLink } from '../cells'
import { formatAttributedCost } from '../shared'
import {
  bucketDays,
  calendarCells,
  chartBucketSize,
  delta,
  harnessShares,
  punchCard,
  ratio,
} from '../insights'

const WEEKDAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday']

// A tab is one linear layout read top to bottom; splitting it would scatter the sections.
// oxlint-disable-next-line eslint/max-lines-per-function
export default function MyWorkTab({
  report,
  name,
}: {
  report: AttributionReportView
  name: string
}) {
  const { summary, previous_summary: previous } = report
  // A truncated previous window makes every change against it wrong, so none are shown.
  const deltaOf = report.previous_truncated ? () => null : delta
  const buckets = bucketDays(report.daily, chartBucketSize(report.window_days))
  const aiShare = ratio(summary.ai_lines, summary.ai_lines + summary.human_lines)
  const previousShare = ratio(previous.ai_lines, previous.ai_lines + previous.human_lines)
  const cells = calendarCells(report.daily)
  const harnesses = harnessShares(report.changes)
  const harnessTotal = harnesses.reduce((total, row) => total + row.aiLines, 0)
  const recent = [...report.pull_requests]
    .sort((a, b) => b.merged_at.localeCompare(a.merged_at))
    .slice(0, 8)

  return (
    <div className="flex flex-col gap-6">
      <p className="text-muted-foreground text-sm">
        <span className="text-foreground font-medium">{name}</span> · compared with your previous{' '}
        {report.window_days} days, not with anyone else.
      </p>
      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        <TrendKpi
          label="Your AI share"
          value={formatPercent(aiShare)}
          delta={deltaOf(aiShare, previousShare)}
          spark={buckets.map((bucket) => bucket.aiShare)}
          tone={AI_TONE}
          hint="Of attested lines"
        />
        <TrendKpi
          label="Merged PRs"
          value={formatCount(summary.merged_pull_request_count)}
          delta={deltaOf(summary.merged_pull_request_count, previous.merged_pull_request_count)}
          spark={buckets.map((bucket) => bucket.mergedPrs)}
        />
        <TrendKpi
          label="Agent spend"
          value={formatUsd10000(summary.priced_cost_usd_10000)}
          delta={deltaOf(summary.priced_cost_usd_10000, previous.priced_cost_usd_10000)}
          spark={buckets.map((bucket) => bucket.committedCost + bucket.uncommittedCost)}
          hint={`${formatPercent(ratio(report.totals.uncommitted_cost_usd_10000, summary.priced_cost_usd_10000))} never committed`}
        />
        <TrendKpi
          label="First commit to merge"
          value={formatHours(
            summary.median_lead_time_seconds == null
              ? null
              : summary.median_lead_time_seconds * 1000,
          )}
          delta={deltaOf(summary.median_lead_time_seconds, previous.median_lead_time_seconds)}
          hint="Median"
        />
      </div>

      <InsightCard
        title="Activity"
        description="Commits per day. A dot marks days where AI wrote most of your attested lines."
      >
        <div className="flex flex-wrap items-start gap-x-10 gap-y-6">
          <ContributionCalendar cells={cells} />
          <CalendarStats cells={cells} />
        </div>
      </InsightCard>

      <div className="grid gap-6 xl:grid-cols-[minmax(0,1fr)_24rem]">
        <InsightCard
          title="When you work with agents"
          description="Commits by weekday and hour (UTC). Brighter dots are hours where most commits were AI-written."
        >
          <PunchCard grid={punchCard(report.changes)} />
        </InsightCard>
        <InsightCard title="Harnesses" description="Share of your committed AI lines.">
          {harnesses.length === 0 ? (
            <p className="text-muted-foreground py-6 text-center text-sm">
              No AI lines in this window.
            </p>
          ) : (
            <ul className="flex flex-col gap-3">
              {harnesses.map((row) => (
                <li key={row.harness} className="flex flex-col gap-1">
                  <div className="flex items-center justify-between gap-2 text-sm">
                    <ToolLabel harness={row.harness} />
                    <span className="text-muted-foreground text-xs tabular-nums">
                      {formatCount(row.aiLines)} lines
                    </span>
                  </div>
                  <InlineMeter value={ratio(row.aiLines, harnessTotal)} tone={AI_TONE} />
                </li>
              ))}
            </ul>
          )}
        </InsightCard>
      </div>

      <InsightCard
        title="Your recent merges"
        description="AI share and cost of the pull requests you merged in the window."
      >
        {recent.length === 0 ? (
          <p className="text-muted-foreground py-6 text-center text-sm">
            No merged pull requests in this window.
          </p>
        ) : (
          <ul className="divide-border flex flex-col divide-y">
            {recent.map((pr) => {
              const added = pr.ai_lines + pr.human_lines
              return (
                <li
                  key={`${pr.repository}#${pr.number}`}
                  className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-4 gap-y-2 py-2.5 sm:grid-cols-[minmax(0,1fr)_12rem_6rem]"
                >
                  <div className="min-w-0">
                    <p className="truncate text-sm font-medium" title={pr.title}>
                      {pr.title}
                    </p>
                    <p className="text-muted-foreground flex flex-wrap items-center gap-x-1 text-xs">
                      <PullRequestLink repository={pr.repository} number={pr.number} url={pr.url} />
                      <span className="whitespace-nowrap">· {formatTimestamp(pr.merged_at)}</span>
                    </p>
                  </div>
                  {/* On narrow screens the share bar drops to its own full-width row. */}
                  {added > 0 ? (
                    <div className="order-last col-span-2 flex items-center gap-2 sm:order-none sm:col-span-1">
                      <div className="flex-1">
                        <AiShareBar ai={pr.ai_lines} human={pr.human_lines} size="sm" />
                      </div>
                      <span className="w-9 text-right text-xs tabular-nums">
                        {formatPercent(pr.ai_lines / added)}
                      </span>
                    </div>
                  ) : (
                    <span className="text-muted-foreground order-last col-span-2 text-xs sm:order-none sm:col-span-1">
                      {EMPTY_VALUE}
                    </span>
                  )}
                  <span className="text-right text-sm tabular-nums">
                    {formatAttributedCost(pr.priced_cost_usd_10000, pr.priced_request_count)}
                  </span>
                </li>
              )
            })}
          </ul>
        )}
      </InsightCard>
    </div>
  )
}

function CalendarStats({ cells }: { cells: ReturnType<typeof calendarCells> }) {
  const active = cells.filter((cell) => cell.commits > 0)
  const aiDays = active.filter((cell) => (cell.aiShare ?? 0) >= 0.5)
  const byWeekday = [0, 0, 0, 0, 0, 0, 0]
  for (const cell of cells) byWeekday[cell.weekday] += cell.commits
  const busiest = byWeekday.indexOf(Math.max(...byWeekday))
  const commits = active.reduce((total, cell) => total + cell.commits, 0)
  return (
    <dl className="grid grid-cols-2 gap-x-8 gap-y-4">
      <Stat
        label="Active days"
        value={`${formatCount(active.length)} of ${formatCount(cells.length)}`}
      />
      <Stat label="AI-majority days" value={formatPercent(ratio(aiDays.length, active.length))} />
      <Stat label="Busiest weekday" value={commits > 0 ? WEEKDAYS[busiest] : EMPTY_VALUE} />
      <Stat
        label="Commits per active day"
        value={active.length > 0 ? (commits / active.length).toFixed(1) : EMPTY_VALUE}
      />
    </dl>
  )
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex flex-col gap-0.5">
      <dt className="text-muted-foreground text-xs">{label}</dt>
      <dd className="text-lg font-medium tabular-nums">{value}</dd>
    </div>
  )
}
