import { useState, useTransition, type FormEvent } from 'react'
import { useRouter } from '@tanstack/react-router'
import { ArrowUp01Icon } from '@hugeicons/core-free-icons'
import { toast } from 'sonner'

import { AppIcon } from '@/components/icons/app-icon'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { getErrorMessage } from '@/lib/errors'
import { formatIncreaseLastDay, formatUsd10000 } from '@/lib/format'
import { increaseBudget } from '@/server/admin-data.functions'
import type { BudgetIncreasePeriod, SpendBudgetUserView } from '@/types/api'

import {
  amountInputValue,
  DEFAULT_INCREASE_PERIOD,
  increaseExpiresAt,
  increasePresets,
  maxIncrease10000,
  validateIncreaseAmount,
  type IncreaseEligibility,
} from './-budget-increase-model'
import { formatCadence } from './-budget-model'

// The "Increase budget" row action and its dialog: a temporary top-up that lasts until
// the end of the calendar month or for seven days.

export function IncreaseBudgetButton({
  eligibility,
  onClick,
}: {
  eligibility: IncreaseEligibility
  onClick: () => void
}) {
  const button = (
    <Button
      type="button"
      size="sm"
      variant="outline"
      disabled={!eligibility.enabled}
      onClick={onClick}
    >
      <AppIcon icon={ArrowUp01Icon} size={14} stroke={1.5} data-icon="inline-start" />
      Increase budget
    </Button>
  )
  if (eligibility.enabled) return button
  // Disabled buttons swallow pointer events, so the tooltip hangs off a wrapper.
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span tabIndex={0} className="inline-flex" aria-label={eligibility.reason}>
          {button}
        </span>
      </TooltipTrigger>
      <TooltipContent>{eligibility.reason}</TooltipContent>
    </Tooltip>
  )
}

export function BudgetIncreaseDialog({
  user,
  onClose,
}: {
  /** The user whose budget is being increased, or `null` when closed. */
  user: SpendBudgetUserView | null
  onClose: () => void
}) {
  return (
    <Dialog open={user !== null} onOpenChange={(open) => (open ? null : onClose())}>
      <DialogContent className="sm:max-w-md">
        {user?.budget ? (
          <BudgetIncreaseForm
            key={user.user_id}
            user={user}
            budget={user.budget}
            onClose={onClose}
          />
        ) : null}
      </DialogContent>
    </Dialog>
  )
}

// oxlint-disable-next-line eslint/max-lines-per-function -- one linear form
function BudgetIncreaseForm({
  user,
  budget,
  onClose,
}: {
  user: SpendBudgetUserView
  budget: NonNullable<SpendBudgetUserView['budget']>
  onClose: () => void
}) {
  const router = useRouter()
  const [period, setPeriod] = useState<BudgetIncreasePeriod>(DEFAULT_INCREASE_PERIOD)
  const [amount, setAmount] = useState('')
  const [submitted, setSubmitted] = useState(false)
  const [isPending, startTransition] = useTransition()

  const base10000 = budget.amount_usd_10000
  const max10000 = maxIncrease10000(base10000)
  const validation = validateIncreaseAmount(amount, base10000)
  const showError = !validation.ok && (submitted || amount.trim() !== '')
  const now = new Date()

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setSubmitted(true)
    if (!validation.ok) return
    const amountUsd = validation.amountUsd
    startTransition(async () => {
      try {
        await increaseBudget({ data: { user_id: user.user_id, amount_usd: amountUsd, period } })
      } catch (error) {
        toast.error(getErrorMessage(error))
        return
      }
      toast.success(`Budget increased for ${user.name}`)
      onClose()
      try {
        await router.invalidate()
      } catch {
        toast.error('Budget increased, but the page did not reload')
      }
    })
  }

  return (
    <form className="flex flex-col gap-5" onSubmit={submit}>
      <DialogHeader>
        <DialogTitle>Increase budget</DialogTitle>
        <DialogDescription>
          Temporarily raise {user.name}&apos;s {formatCadence(budget.cadence).toLowerCase()} budget
          of {formatUsd10000(base10000)}. The increase is added on top of the budget and expires
          automatically.
        </DialogDescription>
      </DialogHeader>
      <FieldGroup>
        <Field>
          <FieldLabel id="budget-increase-period-label">Period</FieldLabel>
          <ToggleGroup
            type="single"
            variant="outline"
            value={period}
            onValueChange={(value) => {
              if (value === 'end_of_month' || value === 'seven_days') setPeriod(value)
            }}
            aria-labelledby="budget-increase-period-label"
            className="w-full"
          >
            <ToggleGroupItem value="end_of_month" className="flex-1">
              End of month
            </ToggleGroupItem>
            <ToggleGroupItem value="seven_days" className="flex-1">
              7 days
            </ToggleGroupItem>
          </ToggleGroup>
          <FieldDescription>
            In effect through {formatIncreaseLastDay(increaseExpiresAt(period, now))} (UTC).
          </FieldDescription>
        </Field>
        <Field data-invalid={showError || undefined}>
          <FieldLabel htmlFor="budget-increase-amount">Increase amount (USD)</FieldLabel>
          <div className="grid grid-cols-4 gap-2" role="group" aria-label="Prefill amount">
            {increasePresets(base10000).map(({ percent, amount10000 }) => (
              <Button
                key={percent}
                type="button"
                size="sm"
                variant="secondary"
                aria-label={`Prefill ${percent}% of the budget`}
                onClick={() => setAmount(amountInputValue(amount10000))}
              >
                {formatUsd10000(amount10000)}
              </Button>
            ))}
          </div>
          <Input
            id="budget-increase-amount"
            type="number"
            inputMode="decimal"
            min={0}
            step="any"
            max={max10000 / 10_000}
            value={amount}
            onChange={(event) => setAmount(event.currentTarget.value)}
            placeholder={amountInputValue(base10000)}
            aria-invalid={showError || undefined}
            autoComplete="off"
          />
          {showError && !validation.ok ? (
            <FieldError>{validation.error}</FieldError>
          ) : (
            <FieldDescription>Up to {formatUsd10000(max10000)} (3× the budget).</FieldDescription>
          )}
        </Field>
      </FieldGroup>
      <DialogFooter>
        <Button type="button" variant="ghost" onClick={onClose}>
          Cancel
        </Button>
        <Button type="submit" disabled={isPending}>
          {isPending ? 'Increasing...' : 'Increase budget'}
        </Button>
      </DialogFooter>
    </form>
  )
}
