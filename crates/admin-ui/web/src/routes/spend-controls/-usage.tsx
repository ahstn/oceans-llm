import { Badge } from '@/components/ui/badge'
import { Progress } from '@/components/ui/progress'
import { CURRENCY_FORMATTER } from '@/lib/format'
import { cn } from '@/lib/utils'

import {
  budgetSourceLabel,
  isInheritedBudgetSource,
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
}

/**
 * - `idle`: no budget and no spend in the window.
 * - `no_budget`: spending without a budget.
 * - `low`: under `LOW_USAGE_RATIO` of the budget.
 */
export type UsageStatus = 'idle' | 'no_budget' | 'low' | 'on_track' | 'warning' | 'over'

export type BudgetUsage = {
  /** Spend divided by budget. `null` when no budget is set. */
  ratio: number | null
  status: UsageStatus
  spendUsd: number
  budgetUsd: number | null
  remainingUsd: number | null
}

export function budgetUsage(owner: BudgetOwner): BudgetUsage {
  const spendUsd = owner.current_window_spend_usd_10000 / 10_000
  if (!owner.budget || owner.budget.amount_usd_10000 <= 0) {
    return {
      ratio: null,
      status: spendUsd === 0 ? 'idle' : 'no_budget',
      spendUsd,
      budgetUsd: null,
      remainingUsd: null,
    }
  }
  const budgetUsd = owner.budget.amount_usd_10000 / 10_000
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

/** Owners that need no attention right now: idle, or well under their budget. */
export function isQuietUsage(usage: BudgetUsage) {
  return usage.status === 'idle' || usage.status === 'low'
}

export type BudgetSummary = {
  totalSpendUsd: number
  budgetedUsers: number
  overBudget: number
  nearLimit: number
}

export function summarizeUsage(usages: readonly BudgetUsage[]): BudgetSummary {
  return usages.reduce<BudgetSummary>(
    (summary, usage) => ({
      totalSpendUsd: summary.totalSpendUsd + usage.spendUsd,
      budgetedUsers: summary.budgetedUsers + (usage.ratio === null ? 0 : 1),
      overBudget: summary.overBudget + (usage.status === 'over' ? 1 : 0),
      nearLimit: summary.nearLimit + (usage.status === 'warning' ? 1 : 0),
    }),
    { totalSpendUsd: 0, budgetedUsers: 0, overBudget: 0, nearLimit: 0 },
  )
}

// ---------------------------------------------------------------------------
// Presentation
// ---------------------------------------------------------------------------

const STATUS_LABEL: Record<UsageStatus, string> = {
  idle: 'Idle',
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
  idle: 'outline',
  no_budget: 'outline',
  low: 'secondary',
  on_track: 'secondary',
  warning: 'warning',
  over: 'destructive',
}

const STATUS_BAR_CLASS: Record<UsageStatus, string> = {
  idle: '',
  no_budget: '',
  low: '[&>[data-slot=progress-indicator]]:bg-muted-foreground/60',
  on_track: '',
  warning: '[&>[data-slot=progress-indicator]]:bg-[var(--color-warning)]',
  over: '[&>[data-slot=progress-indicator]]:bg-destructive',
}

export function UsageStatusBadge({ status }: { status: UsageStatus }) {
  return <Badge variant={STATUS_BADGE_VARIANT[status]}>{STATUS_LABEL[status]}</Badge>
}

/** Marks budgets that come from configuration. Renders nothing for manual budgets. */
export function BudgetSourceBadge({ source }: { source: BudgetSource | null | undefined }) {
  if (!isInheritedBudgetSource(source)) return null
  return <Badge variant="outline">{budgetSourceLabel(source.kind)}</Badge>
}

export function UsageBar({ usage }: { usage: BudgetUsage }) {
  if (usage.ratio === null) {
    return (
      <div className="flex min-w-0 flex-col gap-1">
        <span className="text-sm font-medium">{CURRENCY_FORMATTER.format(usage.spendUsd)}</span>
        <span className="text-muted-foreground text-xs">No budget set</span>
      </div>
    )
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
        className={cn('h-1.5', STATUS_BAR_CLASS[usage.status])}
      />
    </div>
  )
}
