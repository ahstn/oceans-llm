import { formatCount } from '@/lib/format'
import type { AttributionModelView } from '@/types/api'

import { BarListEmpty, BarListRow } from '../-usage-costs/bar-list'
import { formatShare } from '../-usage-costs/shared'
import { GapsBadge } from './cells'
import { formatAttributedCost } from './shared'

/** The breakdown shows the busiest models only, like the Usage Costs breakdowns. */
const MODEL_LIMIT = 10

/** Gateway models behind the window's commits, ranked by allocated requests (the API's order). */
export function ModelBreakdown({ models }: { models: AttributionModelView[] }) {
  const total = models.reduce((sum, model) => sum + model.request_count, 0)
  const top = models.slice(0, MODEL_LIMIT)
  const max = top[0]?.request_count ?? 0
  if (top.length === 0) {
    return <BarListEmpty>No model requests in this window.</BarListEmpty>
  }
  return (
    <ol aria-label="Models" className="flex flex-col gap-1">
      {top.map((model) => (
        <BarListRow
          key={model.model_key}
          label={model.model_key}
          mono
          badge={
            <GapsBadge gaps={model.unpriced_request_count + model.usage_missing_request_count} />
          }
          detail={modelDetail(model, total)}
          value={formatAttributedCost(model.priced_cost_usd_10000, model.priced_request_count)}
          progress={max > 0 ? (model.request_count / max) * 100 : 0}
        />
      ))}
    </ol>
  )
}

/** Allocated requests and their share, plus any requests made after the session's last commit. */
function modelDetail(model: AttributionModelView, totalRequests: number) {
  const detail = `${formatCount(model.request_count)} requests · ${formatShare(model.request_count, totalRequests)}`
  return model.uncommitted_request_count > 0
    ? `${detail} · ${formatCount(model.uncommitted_request_count)} uncommitted`
    : detail
}
