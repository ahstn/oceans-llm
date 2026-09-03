import { Children, type Dispatch, type FormEvent, type ReactNode, type SetStateAction } from 'react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import type { BudgetAlertHistoryItemView } from '@/types/api'

import {
  INHERITED_BUDGET_WARNING,
  badgeVariantForAlert,
  budgetSourceLabel,
  formatOwnerKind,
  formatThreshold,
  formatUsd10000,
  isInheritedBudgetSource,
  type BudgetSettings,
  type BudgetSettingsForm,
  type BudgetSource,
} from './-utils'

const BUDGET_GRID = 'grid grid-cols-[minmax(0,1fr)_170px_170px_220px_180px]'
const ALERT_GRID = 'grid grid-cols-[minmax(0,1fr)_120px_120px_220px_160px]'

export function BudgetTable({
  title,
  description,
  columns,
  emptyMessage,
  children,
}: {
  title: string
  description: string
  columns: string[]
  emptyMessage: string
  children: ReactNode
}) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>{title}</CardTitle>
        <CardDescription>{description}</CardDescription>
      </CardHeader>
      <CardContent>
        <div className="overflow-hidden rounded-md border border-[color:var(--color-border)]">
          <div
            className={`${BUDGET_GRID} bg-[color:var(--color-surface-muted)] text-[var(--color-text-soft)]`}
          >
            {columns.map((column) => (
              <span key={column} className="px-3 py-2 font-semibold">
                {column}
              </span>
            ))}
          </div>
          {Children.count(children) === 0 ? (
            <div className="px-3 py-6 text-sm text-[var(--color-text-soft)]">{emptyMessage}</div>
          ) : (
            children
          )}
        </div>
      </CardContent>
    </Card>
  )
}

export function BudgetRow({ children }: { children: ReactNode }) {
  return (
    <div className={`${BUDGET_GRID} border-t border-[color:var(--color-border)]`}>{children}</div>
  )
}

export function IdentityCell({ primary, secondary }: { primary: string; secondary: string }) {
  return (
    <div className="min-w-0 px-3 py-3">
      <p className="truncate text-sm font-semibold text-[var(--color-text)]">{primary}</p>
      <p className="truncate text-xs text-[var(--color-text-soft)]">{secondary}</p>
    </div>
  )
}

export function BudgetCell({
  budget,
  source,
}: {
  budget?: BudgetSettings | null
  source?: BudgetSource | null
}) {
  return (
    <div className="flex flex-col items-start gap-1 px-3 py-3">
      {budget ? (
        <Badge>{formatUsd10000(budget.amount_usd_10000)}</Badge>
      ) : (
        <span className="text-sm text-[var(--color-text-soft)]">Not set</span>
      )}
      {budget && source ? <Badge variant="outline">{budgetSourceLabel(source.kind)}</Badge> : null}
    </div>
  )
}

export function MoneyCell({ amountUsd10000 }: { amountUsd10000: number }) {
  return (
    <span className="px-3 py-3 text-sm text-[var(--color-text-muted)]">
      {formatUsd10000(amountUsd10000)}
    </span>
  )
}

export function TextCell({
  children,
  tone = 'default',
}: {
  children: ReactNode
  tone?: 'default' | 'danger'
}) {
  return (
    <div className="px-3 py-3">
      <p
        className={
          tone === 'danger'
            ? 'truncate text-sm text-[var(--color-danger)]'
            : 'truncate text-sm text-[var(--color-text)]'
        }
      >
        {children}
      </p>
    </div>
  )
}

export function ActionCell({ children }: { children: ReactNode }) {
  return <div className="flex items-center gap-2 px-3 py-3">{children}</div>
}

export function BudgetSettingsFields({
  form,
  setForm,
}: {
  form: BudgetSettingsForm
  setForm: Dispatch<SetStateAction<BudgetSettingsForm>>
}) {
  return (
    <>
      <div className="grid gap-1">
        <label
          className="text-xs font-semibold text-[var(--color-text-soft)]"
          htmlFor="budget-cadence"
        >
          Cadence
        </label>
        <Select
          value={form.cadence}
          onValueChange={(value) => setForm((current) => ({ ...current, cadence: value }))}
        >
          <SelectTrigger id="budget-cadence">
            <SelectValue placeholder="Cadence" />
          </SelectTrigger>
          <SelectContent>
            <SelectGroup>
              <SelectItem value="daily">Daily</SelectItem>
              <SelectItem value="weekly">Weekly</SelectItem>
              <SelectItem value="monthly">Monthly</SelectItem>
            </SelectGroup>
          </SelectContent>
        </Select>
      </div>

      <div className="grid gap-1">
        <label
          className="text-xs font-semibold text-[var(--color-text-soft)]"
          htmlFor="budget-amount"
        >
          Amount (USD)
        </label>
        <Input
          id="budget-amount"
          value={form.amount_usd}
          onChange={({ currentTarget: { value } }) =>
            setForm((current) => ({ ...current, amount_usd: value }))
          }
          placeholder="100.0000"
          autoComplete="off"
        />
      </div>

      <div className="grid gap-1">
        <label
          className="text-xs font-semibold text-[var(--color-text-soft)]"
          htmlFor="budget-timezone"
        >
          Timezone
        </label>
        <Input
          id="budget-timezone"
          value={form.timezone ?? 'UTC'}
          onChange={({ currentTarget: { value } }) =>
            setForm((current) => ({ ...current, timezone: value }))
          }
          placeholder="UTC"
          autoComplete="off"
        />
      </div>

      <label className="mt-1 flex items-center gap-2 text-sm text-[var(--color-text)]">
        <input
          type="checkbox"
          checked={form.hard_limit}
          onChange={({ currentTarget: { checked } }) =>
            setForm((current) => ({ ...current, hard_limit: checked }))
          }
        />
        Enforce hard limit
      </label>
    </>
  )
}

export function BudgetDialog({
  open,
  label,
  source,
  form,
  setForm,
  isPending,
  onClose,
  onSubmit,
}: {
  open: boolean
  label: string | null
  source: BudgetSource | null | undefined
  form: BudgetSettingsForm
  setForm: Dispatch<SetStateAction<BudgetSettingsForm>>
  isPending: boolean
  onClose: () => void
  onSubmit: (event: FormEvent<HTMLFormElement>) => void
}) {
  return (
    <Dialog open={open} onOpenChange={(nextOpen) => (!nextOpen ? onClose() : null)}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Configure Budget</DialogTitle>
          <DialogDescription>
            Update cadence, limit amount, and hard-limit behavior for {label ?? 'selected scope'}.
          </DialogDescription>
        </DialogHeader>
        <form className="flex flex-col gap-3" onSubmit={onSubmit}>
          {isInheritedBudgetSource(source) ? (
            <p className="rounded-md bg-[var(--color-warning-soft)] px-3 py-2 text-sm text-[var(--color-warning)]">
              {INHERITED_BUDGET_WARNING}
            </p>
          ) : null}
          <BudgetSettingsFields form={form} setForm={setForm} />
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" disabled={isPending}>
              {isPending ? 'Saving...' : 'Save budget'}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

export function AlertHistoryCard({ alerts }: { alerts: BudgetAlertHistoryItemView[] }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Budget Alert History</CardTitle>
        <CardDescription>
          The latest threshold alerts and delivery outcomes for audit review.
        </CardDescription>
      </CardHeader>
      <CardContent>
        <div className="overflow-hidden rounded-md border border-[color:var(--color-border)]">
          <div
            className={`${ALERT_GRID} bg-[color:var(--color-surface-muted)] text-[var(--color-text-soft)]`}
          >
            <span className="px-3 py-2 font-semibold">Owner</span>
            <span className="px-3 py-2 font-semibold">Threshold</span>
            <span className="px-3 py-2 font-semibold">Remaining</span>
            <span className="px-3 py-2 font-semibold">Recipients</span>
            <span className="px-3 py-2 font-semibold">Status</span>
          </div>
          {alerts.length === 0 ? (
            <div className="px-3 py-6 text-sm text-[var(--color-text-soft)]">
              No budget alerts have been recorded yet.
            </div>
          ) : (
            alerts.map((alert) => (
              <div
                key={alert.budget_alert_id}
                className={`${ALERT_GRID} border-t border-[color:var(--color-border)]`}
              >
                <IdentityCell
                  primary={alert.owner_name}
                  secondary={`${formatOwnerKind(alert.owner_kind)} / ${new Date(
                    alert.created_at,
                  ).toLocaleString()}`}
                />
                <TextCell>{formatThreshold(alert)}</TextCell>
                <MoneyCell amountUsd10000={alert.remaining_budget_usd_10000} />
                <TextCell>{alert.recipient_summary}</TextCell>
                <div className="px-3 py-3">
                  <Badge variant={badgeVariantForAlert(alert)}>{alert.delivery_status}</Badge>
                  {alert.failure_reason ? (
                    <p className="mt-1 line-clamp-2 text-xs text-[var(--color-danger)]">
                      {alert.failure_reason}
                    </p>
                  ) : null}
                </div>
              </div>
            ))
          )}
        </div>
      </CardContent>
    </Card>
  )
}
