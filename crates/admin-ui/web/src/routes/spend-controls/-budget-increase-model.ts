import { formatUsd10000 } from '@/lib/format'
import type { AuthSessionView, BudgetIncreasePeriod, SpendBudgetUserView } from '@/types/api'

import { normalizeBudgetAmount } from './-budget-model'
import type { BudgetUsage } from './-usage'

// Temporary budget increases: who may grant one, when, and how much. The gateway
// enforces the same rules; these mirror them so the dialog can explain itself.

/** Spend must reach this share of the effective limit (within 15%) before an increase. */
export const INCREASE_THRESHOLD_RATIO = 0.85
/** An increase may be at most this multiple of the base budget. */
export const MAX_INCREASE_MULTIPLIER = 3
/** Prefill buttons, as percentages of the base budget. */
export const INCREASE_PRESET_PERCENTS = [50, 100, 150, 200] as const

export const DEFAULT_INCREASE_PERIOD: BudgetIncreasePeriod = 'end_of_month'

export type IncreaseEligibility = { enabled: true } | { enabled: false; reason: string }

/**
 * Whether `session` may increase `user`'s budget right now. Platform admins may
 * increase anyone's; team owners and admins only their own team members'.
 */
export function increaseEligibility(
  session: AuthSessionView | null | undefined,
  user: SpendBudgetUserView,
  usage: BudgetUsage,
): IncreaseEligibility {
  if (!canManageUserIncrease(session, user)) {
    return {
      enabled: false,
      reason: "Only platform admins and this user's team admins can increase budgets.",
    }
  }
  if (!user.budget || usage.ratio === null) {
    return { enabled: false, reason: 'This user has no budget to increase.' }
  }
  if (usage.ratio < INCREASE_THRESHOLD_RATIO) {
    return {
      enabled: false,
      reason: 'Available once spend is within 15% of the limit.',
    }
  }
  return { enabled: true }
}

function canManageUserIncrease(
  session: AuthSessionView | null | undefined,
  user: SpendBudgetUserView,
) {
  if (!session) return false
  if (session.user.global_role === 'platform_admin') return true
  const isTeamAdmin = session.team_role === 'owner' || session.team_role === 'admin'
  return isTeamAdmin && session.team_id != null && session.team_id === user.team_id
}

/** Preset increase amounts, in 1/10000 USD, for a base budget in the same unit. */
export function increasePresets(baseAmount10000: number) {
  return INCREASE_PRESET_PERCENTS.map((percent) => ({
    percent,
    amount10000: Math.floor((baseAmount10000 * percent) / 100),
  }))
}

export function maxIncrease10000(baseAmount10000: number) {
  return baseAmount10000 * MAX_INCREASE_MULTIPLIER
}

/** Renders a 1/10000 USD amount for the input: "75", "112.5", never "75.0000". */
export function amountInputValue(amount10000: number) {
  const whole = Math.trunc(amount10000 / 10_000)
  const fraction = String(amount10000 % 10_000)
    .padStart(4, '0')
    .replace(/0+$/, '')
  return fraction ? `${whole}.${fraction}` : String(whole)
}

export type IncreaseAmountResult =
  | { ok: true; amountUsd: string; amount10000: number }
  | { ok: false; error: string }

export function validateIncreaseAmount(
  value: string,
  baseAmount10000: number,
): IncreaseAmountResult {
  const amountUsd = normalizeBudgetAmount(value)
  if (amountUsd === null) {
    return {
      ok: false,
      error: 'Enter an amount greater than 0 with at most four decimal places.',
    }
  }
  const amount10000 = Number(amountUsd.replace('.', ''))
  const max = maxIncrease10000(baseAmount10000)
  if (amount10000 > max) {
    return {
      ok: false,
      error: `The increase cannot exceed ${formatUsd10000(max)} (${MAX_INCREASE_MULTIPLIER}× the budget).`,
    }
  }
  return { ok: true, amountUsd, amount10000 }
}

/** When an increase granted at `now` would expire. Mirrors the gateway (UTC). */
export function increaseExpiresAt(period: BudgetIncreasePeriod, now: Date) {
  if (period === 'seven_days') {
    return new Date(now.getTime() + 7 * 24 * 60 * 60 * 1000)
  }
  return new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth() + 1, 1))
}

const EXPIRY_FORMATTER = new Intl.DateTimeFormat('en-US', {
  month: 'short',
  day: 'numeric',
  timeZone: 'UTC',
})

/** The last day an increase is in effect; expiry is the exclusive boundary. */
export function formatIncreaseLastDay(expiresAt: Date | string) {
  return EXPIRY_FORMATTER.format(new Date(new Date(expiresAt).getTime() - 1))
}
