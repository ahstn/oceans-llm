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
import { Field, FieldGroup, FieldLabel } from '@/components/ui/field'
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
  getSpendBudgets,
  removeTeamBudget,
  removeUserBudget,
  saveTeamBudget,
  saveUserBudget,
} from '@/server/admin-data.functions'
import type {
  SpendBudgetTeamView,
  SpendBudgetUserView,
  SpendBudgetsView,
  UpsertBudgetInput,
} from '@/types/api'

export const Route = createFileRoute('/spend-controls')({
  beforeLoad: ({ location }) => requireAdminSession(location),
  loader: () => getSpendBudgets(),
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
    data: { users, teams },
  } = Route.useLoaderData() as { data: SpendBudgetsView }
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
            Configure hard-limit budgets for user-owned and team-owned spend scopes.
          </CardDescription>
        </CardHeader>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>User Budgets</CardTitle>
          <CardDescription>Per-user budget configuration and current window spend.</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="border-border overflow-hidden rounded-md border">
            <div className="bg-muted text-muted-foreground/80 grid grid-cols-[minmax(0,1fr)_170px_170px_180px]">
              <span className="px-3 py-2 font-semibold">User</span>
              <span className="px-3 py-2 font-semibold">Budget</span>
              <span className="px-3 py-2 font-semibold">Current spend</span>
              <span className="px-3 py-2 font-semibold">Actions</span>
            </div>
            {users.map((user) => (
              <div
                key={user.user_id}
                className="border-border grid grid-cols-[minmax(0,1fr)_170px_170px_180px] border-t"
              >
                <div className="min-w-0 px-3 py-3">
                  <p className="text-foreground truncate text-sm font-semibold">{user.name}</p>
                  <p className="text-muted-foreground/80 truncate text-xs">{user.email}</p>
                </div>
                <div className="px-3 py-3">
                  {user.budget ? (
                    <Badge>
                      {CURRENCY_FORMATTER.format(user.budget.amount_usd_10000 / 10_000)}
                    </Badge>
                  ) : (
                    <span className="text-muted-foreground/80 text-sm">Not set</span>
                  )}
                </div>
                <span className="text-muted-foreground px-3 py-3 text-sm">
                  {CURRENCY_FORMATTER.format(user.current_window_spend_usd_10000 / 10_000)}
                </span>
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
          <CardDescription>Team hard limits for team-owned API key spend.</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="border-border overflow-hidden rounded-md border">
            <div className="bg-muted text-muted-foreground/80 grid grid-cols-[minmax(0,1fr)_170px_170px_180px]">
              <span className="px-3 py-2 font-semibold">Team</span>
              <span className="px-3 py-2 font-semibold">Budget</span>
              <span className="px-3 py-2 font-semibold">Current spend</span>
              <span className="px-3 py-2 font-semibold">Actions</span>
            </div>
            {teams.map((team) => (
              <div
                key={team.team_id}
                className="border-border grid grid-cols-[minmax(0,1fr)_170px_170px_180px] border-t"
              >
                <div className="min-w-0 px-3 py-3">
                  <p className="text-foreground truncate text-sm font-semibold">{team.team_name}</p>
                  <p className="text-muted-foreground/80 truncate text-xs">{team.team_key}</p>
                </div>
                <div className="px-3 py-3">
                  {team.budget ? (
                    <Badge>
                      {CURRENCY_FORMATTER.format(team.budget.amount_usd_10000 / 10_000)}
                    </Badge>
                  ) : (
                    <span className="text-muted-foreground/80 text-sm">Not set</span>
                  )}
                </div>
                <span className="text-muted-foreground px-3 py-3 text-sm">
                  {CURRENCY_FORMATTER.format(team.current_window_spend_usd_10000 / 10_000)}
                </span>
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
          <form onSubmit={handleSave}>
            <FieldGroup className="py-4">
              <Field>
                <FieldLabel htmlFor="budget-cadence">Cadence</FieldLabel>
                <Select
                  value={form.cadence}
                  onValueChange={(value) =>
                    setForm((current) => ({ ...current, cadence: value as 'daily' | 'weekly' }))
                  }
                >
                  <SelectTrigger id="budget-cadence">
                    <SelectValue placeholder="Cadence" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                      <SelectItem value="daily">Daily</SelectItem>
                      <SelectItem value="weekly">Weekly</SelectItem>
                    </SelectGroup>
                  </SelectContent>
                </Select>
              </Field>

              <Field>
                <FieldLabel htmlFor="budget-amount">Amount (USD)</FieldLabel>
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
              </Field>

              <Field>
                <FieldLabel htmlFor="budget-timezone">Timezone</FieldLabel>
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
              </Field>

              <label className="text-foreground mt-2 flex items-center gap-2 text-sm">
                <input
                  type="checkbox"
                  className="accent-primary border-border h-4 w-4 rounded"
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
            </FieldGroup>

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
