import { Badge } from '@/components/ui/badge'
import { Progress } from '@/components/ui/progress'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { CURRENCY_FORMATTER, formatIncreaseLastDay, formatUsd10000 } from '@/lib/format'
import { cn } from '@/lib/utils'
import type { BudgetIncreaseView } from '@/types/api'

import {
  budgetSourceLabel,
  formatCadence,
  type BudgetSettings,
  type BudgetSource,
} from './-budget-model'

// Usage: how one owner's current window spend compares with its budget, and how that
// state is drawn in the tables and summary strip.

export const LOW_USAGE_RATIO = 0.2
export const WARNING_USAGE_RATIO = 0.8

export const PERCENT_FORMATTER = new Intl.NumberFormat('en-US', {
  style: 'percent',
  maximumFractionDigits: 0,
})

export type BudgetOwner = {
  budget?: BudgetSettings | null
  current_window_spend_usd_10000: number
  /** A temporary top-up; only user budgets carry one. */
  active_increase?: Pick<BudgetIncreaseView, 'amount_usd_10000'> | null
}

/**
 * `no_budget` owners have no usable spend figure: the API only sums usage inside an
 * active budget's window, so unbudgeted owners always report zero.
 */
export type UsageStatus = 'no_budget' | 'low' | 'on_track' | 'warning' | 'over'

export type BudgetUsage = {
  /** Spend divided by the effective limit. `null` when no budget is set. */
  ratio: number | null
  status: UsageStatus
  spendUsd: number
  /** The effective limit: the budget plus any active increase. */
  budgetUsd: number | null
  remainingUsd: number | null
}

export function budgetUsage(owner: BudgetOwner): BudgetUsage {
  const spendUsd = owner.current_window_spend_usd_10000 / 10_000
  if (!owner.budget || owner.budget.amount_usd_10000 <= 0) {
    return {
      ratio: null,
      status: 'no_budget',
      spendUsd,
      budgetUsd: null,
      remainingUsd: null,
    }
  }
  const increase10000 = owner.active_increase?.amount_usd_10000 ?? 0
  const budgetUsd = (owner.budget.amount_usd_10000 + increase10000) / 10_000
  const ratio = spendUsd / budgetUsd
  return {
    ratio,
    status: usageStatus(ratio),
    spendUsd,
    budgetUsd,
    remainingUsd: Math.max(0, budgetUsd - spendUsd),
  }
}

function usageStatus(ratio: number): UsageStatus {
  if (ratio >= 1) return 'over'
  if (ratio >= WARNING_USAGE_RATIO) return 'warning'
  if (ratio >= LOW_USAGE_RATIO) return 'on_track'
  return 'low'
}

/** Owners that need no attention right now: well under their budget. */
export function isQuietUsage(usage: BudgetUsage) {
  return usage.status === 'low'
}

export type BudgetSummary = {
  /** Spend counted against active budgets, each in its own current window. */
  budgetedSpendUsd: number
  budgeted: number
  overBudget: number
  nearLimit: number
}

export function summarizeUsage(usages: readonly BudgetUsage[]): BudgetSummary {
  return usages.reduce<BudgetSummary>(
    (summary, usage) => ({
      budgetedSpendUsd: summary.budgetedSpendUsd + usage.spendUsd,
      budgeted: summary.budgeted + (usage.ratio === null ? 0 : 1),
      overBudget: summary.overBudget + (usage.status === 'over' ? 1 : 0),
      nearLimit: summary.nearLimit + (usage.status === 'warning' ? 1 : 0),
    }),
    { budgetedSpendUsd: 0, budgeted: 0, overBudget: 0, nearLimit: 0 },
  )
}

// ---------------------------------------------------------------------------
// Presentation
// ---------------------------------------------------------------------------

const STATUS_LABEL: Record<UsageStatus, string> = {
  no_budget: 'No budget',
  low: 'Low usage',
  on_track: 'On track',
  warning: 'Near limit',
  over: 'Over budget',
}

const STATUS_BADGE_VARIANT: Record<
  UsageStatus,
  'outline' | 'secondary' | 'warning' | 'destructive'
> = {
  no_budget: 'outline',
  low: 'secondary',
  on_track: 'secondary',
  warning: 'warning',
  over: 'destructive',
}

export function UsageStatusBadge({ status }: { status: UsageStatus }) {
  return <Badge variant={STATUS_BADGE_VARIANT[status]}>{STATUS_LABEL[status]}</Badge>
}

// Focusable tooltip triggers: keyboard users reach the detail too.
const TOOLTIP_TRIGGER_CLASS =
  'focus-visible:ring-ring/50 cursor-default rounded-sm outline-none focus-visible:ring-3'

/**
 * The budget amount and its cadence. Enforcement and source sit in the amount's
 * tooltip and accessible name; only a soft limit, the exception, shows in the pill.
 */
export function BudgetAmount({
  budget,
  source,
}: {
  budget: BudgetSettings
  source: BudgetSource | null | undefined
}) {
  const enforcement = budget.hard_limit
    ? 'Hard limit: blocks requests once spent'
    : 'Soft limit: alerts only, never blocks requests'
  const detail = source ? `${enforcement}. Source: ${budgetSourceLabel(source.kind)}` : enforcement
  return (
    <>
      <Tooltip>
        <TooltipTrigger asChild>
          <span
            tabIndex={0}
            className={cn('font-medium tabular-nums', TOOLTIP_TRIGGER_CLASS)}
            data-testid="budget-amount"
          >
            {formatUsd10000(budget.amount_usd_10000)}
            <span className="sr-only">. {detail}.</span>
          </span>
        </TooltipTrigger>
        <TooltipContent>{detail}</TooltipContent>
      </Tooltip>
      <Badge variant="secondary">
        {formatCadence(budget.cadence)}
        {budget.hard_limit ? null : ' · soft'}
      </Badge>
    </>
  )
}

/**
 * The Status cell. An active increase replaces the status pill: the increase is what
 * needs explaining, and the usage bar already carries the status colour. The status
 * stays in the pill's accessible name and tooltip.
 */
export function BudgetStatus({
  usage,
  increase,
}: {
  usage: BudgetUsage
  increase?: Pick<BudgetIncreaseView, 'amount_usd_10000' | 'expires_at'> | null
}) {
  if (!increase) return <UsageStatusBadge status={usage.status} />
  const amount = formatUsd10000(increase.amount_usd_10000)
  const lastDay = formatIncreaseLastDay(increase.expires_at)
  const used =
    usage.ratio === null
      ? ''
      : `: ${PERCENT_FORMATTER.format(usage.ratio)} of ${CURRENCY_FORMATTER.format(usage.budgetUsd ?? 0)} used`
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Badge
          variant="warning"
          tabIndex={0}
          className={TOOLTIP_TRIGGER_CLASS}
          data-testid="budget-increase-status"
        >
          <span className="sr-only">{STATUS_LABEL[usage.status]}. Temporary increase: </span>+
          {amount} until {lastDay}
        </Badge>
      </TooltipTrigger>
      <TooltipContent>
        {STATUS_LABEL[usage.status]}
        {used}, including a temporary {amount} increase through {lastDay} (UTC).
      </TooltipContent>
    </Tooltip>
  )
}

export function UsageBar({ usage }: { usage: BudgetUsage }) {
  if (usage.ratio === null) {
    return <span className="text-muted-foreground text-xs">No budget set</span>
  }
  return (
    <div className="flex min-w-0 flex-col gap-1.5">
      <div className="flex items-baseline justify-between gap-2 text-sm">
        <span className="font-medium">{CURRENCY_FORMATTER.format(usage.spendUsd)}</span>
        <span className="text-muted-foreground text-xs">
          of {CURRENCY_FORMATTER.format(usage.budgetUsd ?? 0)}
        </span>
      </div>
      <Progress
        value={Math.min(100, usage.ratio * 100)}
        aria-label={`${PERCENT_FORMATTER.format(usage.ratio)} of budget used`}
        className={cn(
          'h-1.5',
          usage.status === 'low' && '[&>[data-slot=progress-indicator]]:bg-muted-foreground/60',
          usage.status === 'warning' && '[&>[data-slot=progress-indicator]]:bg-warning',
          usage.status === 'over' && '[&>[data-slot=progress-indicator]]:bg-destructive',
        )}
      />
    </div>
  )
}
