import type { ReactNode } from 'react'

import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Progress } from '@/components/ui/progress'
import { Skeleton } from '@/components/ui/skeleton'
import { formatCount, formatUsd10000 } from '@/lib/format'
import type { AttributionReportView } from '@/types/api'

import { PERCENT_FORMATTER } from '../-usage-costs/shared'
import { aiLineShare, formatAttributedCost, pricingCoverage } from './shared'

/** The headline counts for the window: work landed, AI authorship, and what the agents cost. */
export function AttributionKpis({
  report,
  pending,
}: {
  report: AttributionReportView
  pending: boolean
}) {
  const { totals, coverage } = report
  const share = aiLineShare(totals.ai_lines, totals.human_lines)
  const priced = pricingCoverage(coverage)
  return (
    <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3 2xl:grid-cols-6">
      <Kpi
        label="Commits"
        value={formatCount(totals.commit_count)}
        hint={
          totals.unmatched_author_commit_count > 0
            ? `${formatCount(totals.unmatched_author_commit_count)} with an unmatched author`
            : 'Every author matched a user'
        }
        pending={pending}
      />
      <Kpi
        label="Merged PRs"
        value={formatCount(totals.pull_request_count)}
        hint={`Merged in the last ${report.window_days} days`}
        pending={pending}
      />
      <Kpi
        label="AI lines"
        value={formatCount(totals.ai_lines)}
        hint={
          share === null
            ? 'No attested lines'
            : `${PERCENT_FORMATTER.format(share)} of attested lines · ${formatCount(totals.human_lines)} human`
        }
        pending={pending}
      />
      <Kpi
        label="Requests"
        value={formatCount(totals.request_count)}
        hint={
          <span className="flex flex-col gap-0.5">
            <span>
              {formatCount(totals.joined_session_count)} of {formatCount(totals.ai_session_count)}{' '}
              AI sessions joined
            </span>
            <span>{formatCount(totals.uncommitted_request_count)} uncommitted</span>
          </span>
        }
        pending={pending}
      />
      <Kpi
        label="Attributed cost"
        value={formatAttributedCost(totals.priced_cost_usd_10000, totals.priced_request_count)}
        hint={
          totals.uncommitted_cost_usd_10000 > 0
            ? `${formatUsd10000(totals.uncommitted_cost_usd_10000)} in uncommitted work`
            : 'Priced requests only'
        }
        pending={pending}
      />
      <Kpi
        label="Pricing coverage"
        value={PERCENT_FORMATTER.format(priced)}
        hint={
          <span className="flex flex-col gap-1.5">
            <Progress value={priced * 100} aria-label="Pricing coverage" />
            <span>
              {formatCount(coverage.unpriced_request_count)} unpriced ·{' '}
              {formatCount(coverage.usage_missing_request_count)} usage missing
            </span>
          </span>
        }
        tone={priced < 0.95 ? 'warning' : 'default'}
        pending={pending}
      />
    </div>
  )
}

function Kpi({
  label,
  value,
  hint,
  tone = 'default',
  pending,
}: {
  label: string
  value: string
  hint: ReactNode
  tone?: 'default' | 'warning'
  pending: boolean
}) {
  return (
    <Card size="sm">
      <CardHeader>
        <CardDescription>{label}</CardDescription>
        {pending ? (
          <Skeleton className="h-7 w-28" />
        ) : (
          <CardTitle
            className={
              tone === 'warning' ? 'text-warning text-2xl tabular-nums' : 'text-2xl tabular-nums'
            }
          >
            {value}
          </CardTitle>
        )}
      </CardHeader>
      <CardContent className="text-muted-foreground text-xs">{hint}</CardContent>
    </Card>
  )
}
