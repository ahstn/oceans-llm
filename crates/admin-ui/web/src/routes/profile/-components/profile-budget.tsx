import type { CSSProperties } from 'react'

import { Badge } from '@/components/ui/badge'
import { Progress } from '@/components/ui/progress'
import { formatUsd10000 } from '@/lib/format'
import { cn } from '@/lib/utils'
import type { MyProfileBudgetView } from '@/types/api'

import { PERCENT_FORMATTER } from './profile-data'

const CADENCE_LABEL: Record<string, string> = {
  daily: 'Daily',
  weekly: 'Weekly',
  monthly: 'Monthly',
}

export type BudgetStatus = {
  spent: number
  limit: number
  remaining: number
  /** Spent over limit, uncapped so overspend reads as > 100%. */
  ratio: number
  tone: 'ok' | 'warning' | 'over'
}

export function budgetStatus(budget: MyProfileBudgetView): BudgetStatus {
  const spent = budget.spent_usd_10000
  const limit = budget.settings.amount_usd_10000
  const ratio = limit > 0 ? spent / limit : 0
  return {
    spent,
    limit,
    remaining: Math.max(0, limit - spent),
    ratio,
    tone: ratio >= 1 ? 'over' : ratio >= 0.8 ? 'warning' : 'ok',
  }
}

const TONE_COLOR: Record<BudgetStatus['tone'], string> = {
  ok: 'var(--primary)',
  warning: 'var(--color-warning)',
  over: 'var(--destructive)',
}

/**
 * Date and time are formatted separately: the joined date-time pattern differs between the
 * server's and the browser's ICU data, which breaks hydration.
 */
export function formatResetsAt(budget: MyProfileBudgetView) {
  const resetsAt = new Date(budget.period_end)
  const date = resetsAt.toLocaleDateString('en-US', {
    weekday: 'short',
    month: 'short',
    day: 'numeric',
    timeZone: 'UTC',
  })
  const time = `${String(resetsAt.getUTCHours()).padStart(2, '0')}:${String(resetsAt.getUTCMinutes()).padStart(2, '0')}`
  return `${date}, ${time} UTC`
}

export function BudgetCadenceBadge({ budget }: { budget: MyProfileBudgetView }) {
  return (
    <Badge variant="outline">
      {CADENCE_LABEL[budget.settings.cadence] ?? budget.settings.cadence}
      {budget.settings.hard_limit ? ' · hard limit' : ''}
    </Badge>
  )
}

/** Spent-versus-limit bar with the figures a user checks before starting work. */
export function BudgetMeter({
  budget,
  size = 'default',
}: {
  budget: MyProfileBudgetView
  size?: 'default' | 'lg'
}) {
  const status = budgetStatus(budget)
  return (
    <div data-testid="budget-meter" className="flex flex-col gap-2">
      <div className="flex items-baseline justify-between gap-3">
        <div className="flex items-baseline gap-1.5">
          <span
            className={cn(
              'font-semibold tabular-nums',
              size === 'lg' ? 'text-3xl' : 'text-xl',
              status.tone === 'over' && 'text-destructive',
            )}
          >
            {formatUsd10000(status.spent)}
          </span>
          <span className="text-muted-foreground text-sm tabular-nums">
            of {formatUsd10000(status.limit)}
          </span>
        </div>
        <span className="text-muted-foreground text-sm tabular-nums">
          {PERCENT_FORMATTER.format(status.ratio)} used
        </span>
      </div>
      <Progress
        value={Math.min(100, status.ratio * 100)}
        aria-label="Budget used"
        className={cn(
          'bg-[color-mix(in_oklch,var(--bar-tone)_18%,transparent)] [&>[data-slot=progress-indicator]]:bg-[var(--bar-tone)]',
          size === 'lg' ? 'h-2.5' : 'h-2',
        )}
        style={{ '--bar-tone': TONE_COLOR[status.tone] } as CSSProperties}
      />
      <div className="text-muted-foreground flex flex-wrap justify-between gap-x-4 gap-y-1 text-xs">
        <span className="tabular-nums">
          {status.tone === 'over'
            ? `${formatUsd10000(status.spent - status.limit)} over budget`
            : `${formatUsd10000(status.remaining)} remaining`}
        </span>
        <span>Resets {formatResetsAt(budget)}</span>
      </div>
    </div>
  )
}

export function NoBudget() {
  return (
    <p data-testid="no-budget" className="text-muted-foreground text-sm">
      No personal budget is set. Spend is still tracked and counts toward any team budget.
    </p>
  )
}
