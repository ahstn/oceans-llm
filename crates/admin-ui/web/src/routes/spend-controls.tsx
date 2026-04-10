import { useMemo, useState, useTransition, type FormEvent } from 'react'
import { createFileRoute, useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

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
import { requireAdminSession } from '@/routes/-admin-guard'
import {
  getBudgetAlertHistory,
  getSpendBudgets,
  removeTeamBudget,
  removeUserBudget,
  saveTeamBudget,
  saveUserBudget,
} from '@/server/admin-data.functions'
import type {
  BudgetAlertHistoryItemView,
  BudgetAlertHistoryView,
  SpendBudgetTeamView,
  SpendBudgetUserView,
  SpendBudgetsView,
  UpsertBudgetInput,
} from '@/types/api'

export const Route = createFileRoute('/spend-controls')({
  beforeLoad: ({ location }) => requireAdminSession(location),
  loader: async () => {
    const [budgets, alerts] = await Promise.all([
      getSpendBudgets(),
      getBudgetAlertHistory({
        data: { page: 1, page_size: 10, owner_kind: 'all', status: 'all', channel: 'all' },
      }),
    ])
    return { budgets, alerts }
  },
  component: SpendControlsPage,
})

type BudgetDialogState =
  | { mode: 'closed' }
  | { mode: 'user'; user: SpendBudgetUserView }
  | { mode: 'team'; team: SpendBudgetTeamView }

const initialBudgetInput: UpsertBudgetInput = {
  cadence: 'daily',
  amount_usd: '0.0000',
  hard_limit: true,
  timezone: 'UTC',
}

const CURRENCY_FORMATTER = new Intl.NumberFormat('en-US', {
  style: 'currency',
  currency: 'USD',
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
})

export function SpendControlsPage() {
  const router = useRouter()
  const {
    budgets: {
      data: { users, teams },
    },
    alerts: {
      data: { items: alertItems },
    },
  } = Route.useLoaderData() as {
    budgets: { data: SpendBudgetsView }
    alerts: { data: BudgetAlertHistoryView }
  }
  const [dialogState, setDialogState] = useState<BudgetDialogState>({ mode: 'closed' })
  const [form, setForm] = useState<UpsertBudgetInput>(initialBudgetInput)
  const [isPending, startTransition] = useTransition()

  const openLabel = useMemo(() => {
    if (dialogState.mode === 'user') {
      return dialogState.user.name
    }
    if (dialogState.mode === 'team') {
      return dialogState.team.team_name
    }
    return null
  }, [dialogState])

  function openUserDialog(user: SpendBudgetUserView) {
    setDialogState({ mode: 'user', user })
    setForm({
      cadence: user.budget?.cadence ?? 'daily',
      amount_usd: user.budget?.amount_usd ?? '0.0000',
      hard_limit: user.budget?.hard_limit ?? true,
      timezone: user.budget?.timezone ?? 'UTC',
    })
  }

  function openTeamDialog(team: SpendBudgetTeamView) {
    setDialogState({ mode: 'team', team })
    setForm({
      cadence: team.budget?.cadence ?? 'daily',
      amount_usd: team.budget?.amount_usd ?? '0.0000',
      hard_limit: team.budget?.hard_limit ?? true,
      timezone: team.budget?.timezone ?? 'UTC',
    })
  }

  function closeDialog() {
    setDialogState({ mode: 'closed' })
    setForm(initialBudgetInput)
  }

  async function refreshBudgets() {
    await router.invalidate()
  }

  async function handleSave(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (dialogState.mode === 'closed') {
      return
    }

    startTransition(async () => {
      try {
        const payload: UpsertBudgetInput = {
          cadence: form.cadence,
          amount_usd: form.amount_usd,
          hard_limit: form.hard_limit,
          timezone: form.timezone?.trim() || 'UTC',
        }
        if (dialogState.mode === 'user') {
          await saveUserBudget({
            data: {
              userId: dialogState.user.user_id,
              input: payload,
            },
          })
        } else {
          await saveTeamBudget({
            data: {
              teamId: dialogState.team.team_id,
              input: payload,
            },
          })
        }
        toast.success('Budget updated')
        await refreshBudgets()
        closeDialog()
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  async function handleDeactivateUser(user: SpendBudgetUserView) {
    startTransition(async () => {
      try {
        await removeUserBudget({ data: { userId: user.user_id } })
        toast.success('User budget removed')
        await refreshBudgets()
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  async function handleDeactivateTeam(team: SpendBudgetTeamView) {
    startTransition(async () => {
      try {
        await removeTeamBudget({ data: { teamId: team.team_id } })
        toast.success('Team budget removed')
        await refreshBudgets()
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader>
          <CardTitle>Spend Controls</CardTitle>
          <CardDescription>
            Configure hard-limit budgets, review who will receive email alerts, and audit threshold
            notifications.
          </CardDescription>
        </CardHeader>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>User Budgets</CardTitle>
          <CardDescription>
            Per-user budget configuration and current window spend. Budget alerts are delivered to
            the user email on file.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="overflow-hidden rounded-md border border-[color:var(--color-border)]">
            <div className="grid grid-cols-[minmax(0,1fr)_170px_170px_220px_180px] bg-[color:var(--color-surface-muted)] text-[var(--color-text-soft)]">
              <span className="px-3 py-2 font-semibold">User</span>
              <span className="px-3 py-2 font-semibold">Budget</span>
              <span className="px-3 py-2 font-semibold">Current spend</span>
              <span className="px-3 py-2 font-semibold">Alert recipient</span>
              <span className="px-3 py-2 font-semibold">Actions</span>
            </div>
            {users.map((user) => (
              <div
                key={user.user_id}
                className="grid grid-cols-[minmax(0,1fr)_170px_170px_220px_180px] border-t border-[color:var(--color-border)]"
              >
                <div className="min-w-0 px-3 py-3">
                  <p className="truncate text-sm font-semibold text-[var(--color-text)]">
                    {user.name}
                  </p>
                  <p className="truncate text-xs text-[var(--color-text-soft)]">{user.email}</p>
                </div>
                <div className="px-3 py-3">
                  {user.budget ? (
                    <Badge>
                      {CURRENCY_FORMATTER.format(user.budget.amount_usd_10000 / 10_000)}
                    </Badge>
                  ) : (
                    <span className="text-sm text-[var(--color-text-soft)]">Not set</span>
                  )}
                </div>
                <span className="px-3 py-3 text-sm text-[var(--color-text-muted)]">
                  {CURRENCY_FORMATTER.format(user.current_window_spend_usd_10000 / 10_000)}
                </span>
                <div className="px-3 py-3">
                  <p className="truncate text-sm text-[var(--color-text)]">
                    {user.alert_recipient_summary}
                  </p>
                </div>
                <div className="flex items-center gap-2 px-3 py-3">
                  <Button
                    type="button"
                    size="sm"
                    variant="secondary"
                    onClick={() => openUserDialog(user)}
                  >
                    Configure
                  </Button>
                  {user.budget ? (
                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      disabled={isPending}
                      onClick={() => handleDeactivateUser(user)}
                    >
                      Remove
                    </Button>
                  ) : null}
                </div>
              </div>
            ))}
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Team Budgets</CardTitle>
          <CardDescription>
            Team hard limits for team-owned API key spend. Alerts go to active team owners and
            admins only.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="overflow-hidden rounded-md border border-[color:var(--color-border)]">
            <div className="grid grid-cols-[minmax(0,1fr)_170px_170px_220px_180px] bg-[color:var(--color-surface-muted)] text-[var(--color-text-soft)]">
              <span className="px-3 py-2 font-semibold">Team</span>
              <span className="px-3 py-2 font-semibold">Budget</span>
              <span className="px-3 py-2 font-semibold">Current spend</span>
              <span className="px-3 py-2 font-semibold">Alert recipients</span>
              <span className="px-3 py-2 font-semibold">Actions</span>
            </div>
            {teams.map((team) => (
              <div
                key={team.team_id}
                className="grid grid-cols-[minmax(0,1fr)_170px_170px_220px_180px] border-t border-[color:var(--color-border)]"
              >
                <div className="min-w-0 px-3 py-3">
                  <p className="truncate text-sm font-semibold text-[var(--color-text)]">
                    {team.team_name}
                  </p>
                  <p className="truncate text-xs text-[var(--color-text-soft)]">{team.team_key}</p>
                </div>
                <div className="px-3 py-3">
                  {team.budget ? (
                    <Badge>
                      {CURRENCY_FORMATTER.format(team.budget.amount_usd_10000 / 10_000)}
                    </Badge>
                  ) : (
                    <span className="text-sm text-[var(--color-text-soft)]">Not set</span>
                  )}
                </div>
                <span className="px-3 py-3 text-sm text-[var(--color-text-muted)]">
                  {CURRENCY_FORMATTER.format(team.current_window_spend_usd_10000 / 10_000)}
                </span>
                <div className="px-3 py-3">
                  <p
                    className={
                      team.alert_email_ready
                        ? 'truncate text-sm text-[var(--color-text)]'
                        : 'text-sm text-[var(--color-danger)]'
                    }
                  >
                    {team.alert_recipient_summary}
                  </p>
                </div>
                <div className="flex items-center gap-2 px-3 py-3">
                  <Button
                    type="button"
                    size="sm"
                    variant="secondary"
                    onClick={() => openTeamDialog(team)}
                  >
                    Configure
                  </Button>
                  {team.budget ? (
                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      disabled={isPending}
                      onClick={() => handleDeactivateTeam(team)}
                    >
                      Remove
                    </Button>
                  ) : null}
                </div>
              </div>
            ))}
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Budget Alert History</CardTitle>
          <CardDescription>
            The latest threshold alerts and delivery outcomes for audit review.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="overflow-hidden rounded-md border border-[color:var(--color-border)]">
            <div className="grid grid-cols-[minmax(0,1fr)_120px_120px_220px_160px] bg-[color:var(--color-surface-muted)] text-[var(--color-text-soft)]">
              <span className="px-3 py-2 font-semibold">Owner</span>
              <span className="px-3 py-2 font-semibold">Threshold</span>
              <span className="px-3 py-2 font-semibold">Remaining</span>
              <span className="px-3 py-2 font-semibold">Recipients</span>
              <span className="px-3 py-2 font-semibold">Status</span>
            </div>
            {alertItems.length === 0 ? (
              <div className="px-3 py-6 text-sm text-[var(--color-text-soft)]">
                No budget alerts have been recorded yet.
              </div>
            ) : (
              alertItems.map((alert) => (
                <div
                  key={alert.budget_alert_id}
                  className="grid grid-cols-[minmax(0,1fr)_120px_120px_220px_160px] border-t border-[color:var(--color-border)]"
                >
                  <div className="min-w-0 px-3 py-3">
                    <p className="truncate text-sm font-semibold text-[var(--color-text)]">
                      {alert.owner_name}
                    </p>
                    <p className="truncate text-xs text-[var(--color-text-soft)]">
                      {alert.owner_kind} • {new Date(alert.created_at).toLocaleString()}
                    </p>
                  </div>
                  <span className="px-3 py-3 text-sm text-[var(--color-text-muted)]">
                    {formatThreshold(alert)}
                  </span>
                  <span className="px-3 py-3 text-sm text-[var(--color-text-muted)]">
                    {CURRENCY_FORMATTER.format(alert.remaining_budget_usd_10000 / 10_000)}
                  </span>
                  <div className="px-3 py-3">
                    <p className="line-clamp-2 text-sm text-[var(--color-text-muted)]">
                      {alert.recipient_summary}
                    </p>
                  </div>
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

      <Dialog
        open={dialogState.mode !== 'closed'}
        onOpenChange={(open) => (!open ? closeDialog() : null)}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Configure Budget</DialogTitle>
            <DialogDescription>
              Update cadence, limit amount, and hard-limit behavior for{' '}
              {openLabel ?? 'selected owner'}.
            </DialogDescription>
          </DialogHeader>
          <form className="flex flex-col gap-3" onSubmit={handleSave}>
            <div className="grid gap-1">
              <label
                className="text-xs font-semibold text-[var(--color-text-soft)]"
                htmlFor="budget-cadence"
              >
                Cadence
              </label>
              <Select
                value={form.cadence}
                onValueChange={(value) =>
                  setForm((current) => ({
                    ...current,
                    cadence: value as 'daily' | 'weekly' | 'monthly',
                  }))
                }
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
                onChange={(event) =>
                  setForm((current) => ({
                    ...current,
                    amount_usd: event.currentTarget.value,
                  }))
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
                onChange={(event) =>
                  setForm((current) => ({
                    ...current,
                    timezone: event.currentTarget.value,
                  }))
                }
                placeholder="UTC"
                autoComplete="off"
              />
            </div>

            <label className="mt-1 flex items-center gap-2 text-sm text-[var(--color-text)]">
              <input
                type="checkbox"
                checked={form.hard_limit}
                onChange={(event) =>
                  setForm((current) => ({
                    ...current,
                    hard_limit: event.currentTarget.checked,
                  }))
                }
              />
              Enforce hard limit
            </label>

            <DialogFooter>
              <Button type="button" variant="ghost" onClick={closeDialog}>
                Cancel
              </Button>
              <Button type="submit" disabled={isPending}>
                {isPending ? 'Saving...' : 'Save budget'}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </div>
  )
}

function getErrorMessage(error: unknown) {
  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message
  }
  return 'Request failed'
}

function badgeVariantForAlert(
  alert: BudgetAlertHistoryItemView,
): 'default' | 'success' | 'warning' {
  if (alert.delivery_status === 'sent') {
    return 'success'
  }
  if (alert.delivery_status === 'pending') {
    return 'warning'
  }
  return 'default'
}

function formatThreshold(alert: BudgetAlertHistoryItemView) {
  return `${alert.threshold_bps / 100}%`
}
