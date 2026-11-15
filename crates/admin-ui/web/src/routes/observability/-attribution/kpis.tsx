import { KpiCard } from '@/components/data/kpi-card'
import { formatCount } from '@/lib/format'
import type { AttributionReportView } from '@/types/api'

import { PERCENT_FORMATTER } from '../-usage-costs/shared'
import { aiLineShare } from './shared'

/** The headline counts for the window: work landed and AI authorship. */
export function AttributionKpis({
  report,
  pending,
}: {
  report: AttributionReportView
  pending: boolean
}) {
  const { totals } = report
  const share = aiLineShare(totals.ai_lines, totals.human_lines)
  return (
    <div className="grid gap-3 sm:grid-cols-3">
      <KpiCard
        label="Commits"
        value={formatCount(totals.commit_count)}
        hint={
          totals.unmatched_author_commit_count > 0
            ? `${formatCount(totals.unmatched_author_commit_count)} with an unmatched author`
            : 'Every author matched a user'
        }
        pending={pending}
      />
      <KpiCard
        label="Merged PRs"
        value={formatCount(totals.pull_request_count)}
        hint={`Merged in the last ${report.window_days} days`}
        pending={pending}
      />
      <KpiCard
        label="AI lines"
        value={formatCount(totals.ai_lines)}
        hint={
          share === null
            ? 'No attested lines'
            : `${PERCENT_FORMATTER.format(share)} of attested lines · ${formatCount(totals.human_lines)} human`
        }
        pending={pending}
      />
    </div>
  )
}
