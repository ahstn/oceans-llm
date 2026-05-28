import {
  Children,
  useMemo,
  useState,
  useTransition,
  type Dispatch,
  type FormEvent,
  type ReactNode,
  type SetStateAction,
} from 'react'
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
  getModels,
  getSpendBudgets,
  removeBudget,
  saveBudget,
} from '@/server/admin-data.functions'
import type {
  BudgetAlertHistoryItemView,
  BudgetAlertHistoryView,
  BudgetScopeRequest,
  ModelView,
  SpendBudgetServiceAccountView,
  SpendBudgetUserModelView,
  SpendBudgetUserView,
  SpendBudgetsView,
  UpsertBudgetInput,
} from '@/types/api'

export const Route = createFileRoute('/spend-controls')({
  beforeLoad: ({ location }) => requireAdminSession(location),
  loader: async () => {
    const [budgets, alerts, models] = await Promise.all([
      getSpendBudgets(),
      getBudgetAlertHistory({
        data: { page: 1, page_size: 10, owner_kind: 'all', status: 'all', channel: 'all' },
      }),
      getModels({ data: { page: 1, page_size: 200 } }),
    ])
    return { budgets, alerts, models }
  },
  component: SpendControlsPage,
})

type BudgetSettingsForm = Omit<UpsertBudgetInput, 'scope'>

type BudgetDialogState =
  | { mode: 'closed' }
  | { mode: 'user'; user: SpendBudgetUserView }
  | { mode: 'service_account'; serviceAccount: SpendBudgetServiceAccountView }
  | { mode: 'user_model'; budget: SpendBudgetUserModelView }

type UserModelDraft = {
  userId: string
  selectorKind: 'model_id' | 'upstream_model'
  selectorValue: string
  settings: BudgetSettingsForm
}

const initialBudgetSettings: BudgetSettingsForm = {
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
      data: { users, service_accounts: serviceAccounts, user_model_budgets: userModelBudgets },
    },
    alerts: {
      data: { items: alertItems },
    },
    models: {
      data: { items: models },
    },
  } = Route.useLoaderData() as {
    budgets: { data: SpendBudgetsView }
    alerts: { data: BudgetAlertHistoryView }
    models: { data: { items: ModelView[] } }
  }
  const [dialogState, setDialogState] = useState<BudgetDialogState>({ mode: 'closed' })
  const [form, setForm] = useState<BudgetSettingsForm>(initialBudgetSettings)
  const [userModelDraft, setUserModelDraft] = useState<UserModelDraft>(() =>
    createInitialUserModelDraft(users, models),
  )
  const [isPending, startTransition] = useTransition()

  const usersById = useMemo(() => new Map(users.map((user) => [user.user_id, user])), [users])
  const openLabel = useMemo(() => {
    if (dialogState.mode === 'user') {
      return dialogState.user.name
    }
    if (dialogState.mode === 'service_account') {
      return dialogState.serviceAccount.service_account_name
    }
    if (dialogState.mode === 'user_model') {
      const userName = usersById.get(dialogState.budget.user_id)?.name ?? dialogState.budget.user_id
      return `${userName} / ${formatUserModelSelector(dialogState.budget)}`
    }
    return null
  }, [dialogState, usersById])

  function openUserDialog(user: SpendBudgetUserView) {
    setDialogState({ mode: 'user', user })
    setForm(settingsFromBudget(user.budget))
  }

  function openServiceAccountDialog(serviceAccount: SpendBudgetServiceAccountView) {
    setDialogState({ mode: 'service_account', serviceAccount })
    setForm(settingsFromBudget(serviceAccount.budget))
  }

  function openUserModelDialog(budget: SpendBudgetUserModelView) {
    setDialogState({ mode: 'user_model', budget })
    setForm(settingsFromBudget(budget.budget))
  }

  function closeDialog() {
    setDialogState({ mode: 'closed' })
    setForm(initialBudgetSettings)
  }

  async function refreshBudgets() {
    await router.invalidate()
  }

  async function handleSave(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (dialogState.mode === 'closed') {
      return
    }

    const scope = scopeForDialog(dialogState)
    startTransition(async () => {
      try {
        await saveBudget({ data: budgetPayload(scope, form) })
        toast.success('Budget updated')
        await refreshBudgets()
        closeDialog()
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  async function handleCreateUserModelBudget(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const selectorValue = userModelDraft.selectorValue.trim()
    if (!userModelDraft.userId || !selectorValue) {
      toast.error('Select a user and model scope before saving')
      return
    }

    const scope: BudgetScopeRequest =
      userModelDraft.selectorKind === 'model_id'
        ? { kind: 'user_model', user_id: userModelDraft.userId, model_id: selectorValue }
        : { kind: 'user_model', user_id: userModelDraft.userId, upstream_model: selectorValue }

    startTransition(async () => {
      try {
        await saveBudget({ data: budgetPayload(scope, userModelDraft.settings) })
        toast.success('User model budget created')
        await refreshBudgets()
        setUserModelDraft(createInitialUserModelDraft(users, models))
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  async function handleDeactivate(scope: BudgetScopeRequest, message: string) {
    startTransition(async () => {
      try {
        await removeBudget({ data: { scope } })
        toast.success(message)
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
            Configure budgets for human users, service accounts, and user-specific model scopes.
          </CardDescription>
        </CardHeader>
      </Card>

      <BudgetTable
        title="User Budgets"
        description="Per-user budget configuration and current window spend."
        columns={['User', 'Budget', 'Current spend', 'Alert recipient', 'Actions']}
        emptyMessage="No users are available."
      >
        {users.map((user) => (
          <BudgetRow key={user.user_id}>
            <IdentityCell primary={user.name} secondary={user.email} />
            <BudgetCell budget={user.budget} />
            <MoneyCell amountUsd10000={user.current_window_spend_usd_10000} />
            <TextCell>{user.alert_recipient_summary}</TextCell>
            <ActionCell>
              <Button type="button" size="sm" variant="secondary" onClick={() => openUserDialog(user)}>
                Configure
              </Button>
              {user.budget ? (
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  disabled={isPending}
                  onClick={() =>
                    handleDeactivate(
                      { kind: 'user', user_id: user.user_id },
                      'User budget removed',
                    )
                  }
                >
                  Remove
                </Button>
              ) : null}
            </ActionCell>
          </BudgetRow>
        ))}
      </BudgetTable>

      <BudgetTable
        title="Service Account Budgets"
        description="Active service-account keys require an active service-account budget."
        columns={['Service account', 'Budget', 'Current spend', 'Alert recipients', 'Actions']}
        emptyMessage="No service accounts are available."
      >
        {serviceAccounts.map((serviceAccount) => (
          <BudgetRow key={serviceAccount.service_account_id}>
            <IdentityCell
              primary={serviceAccount.service_account_name}
              secondary={`${serviceAccount.service_account_key} / ${serviceAccount.team_name}`}
            />
            <BudgetCell budget={serviceAccount.budget} />
            <MoneyCell amountUsd10000={serviceAccount.current_window_spend_usd_10000} />
            <TextCell tone={serviceAccount.alert_email_ready ? 'default' : 'danger'}>
              {serviceAccount.alert_recipient_summary}
            </TextCell>
            <ActionCell>
              <Button
                type="button"
                size="sm"
                variant="secondary"
                onClick={() => openServiceAccountDialog(serviceAccount)}
              >
                Configure
              </Button>
              {serviceAccount.budget ? (
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  disabled={isPending}
                  onClick={() =>
                    handleDeactivate(
                      {
                        kind: 'service_account',
                        service_account_id: serviceAccount.service_account_id,
                      },
                      'Service account budget removed',
                    )
                  }
                >
                  Remove
                </Button>
              ) : null}
            </ActionCell>
          </BudgetRow>
        ))}
      </BudgetTable>

      <BudgetTable
        title="User Model Budgets"
        description="Model-specific budgets are evaluated before the user's general budget."
        columns={['User', 'Model scope', 'Budget', 'Current spend', 'Actions']}
        emptyMessage="No user model budgets are configured."
      >
        {userModelBudgets.map((budget) => (
          <BudgetRow key={budget.scope_key}>
            <IdentityCell
              primary={usersById.get(budget.user_id)?.name ?? budget.user_id}
              secondary={usersById.get(budget.user_id)?.email ?? budget.user_id}
            />
            <TextCell>{formatUserModelSelector(budget)}</TextCell>
            <BudgetCell budget={budget.budget} />
            <MoneyCell amountUsd10000={budget.current_window_spend_usd_10000} />
            <ActionCell>
              <Button
                type="button"
                size="sm"
                variant="secondary"
                onClick={() => openUserModelDialog(budget)}
              >
                Configure
              </Button>
              <Button
                type="button"
                size="sm"
                variant="ghost"
                disabled={isPending}
                onClick={() =>
                  handleDeactivate(scopeForUserModelBudget(budget), 'User model budget removed')
                }
              >
                Remove
              </Button>
            </ActionCell>
          </BudgetRow>
        ))}
      </BudgetTable>

      <Card>
        <CardHeader>
          <CardTitle>Add User Model Budget</CardTitle>
          <CardDescription>
            Create a budget for one user and either a managed model id or an upstream model name.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form className="grid gap-3 lg:grid-cols-[220px_190px_minmax(0,1fr)_140px_130px_120px_120px]" onSubmit={handleCreateUserModelBudget}>
            <Select
              value={userModelDraft.userId}
              onValueChange={(value) =>
                setUserModelDraft((current) => ({ ...current, userId: value }))
              }
            >
              <SelectTrigger>
                <SelectValue placeholder="User" />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  {users.map((user) => (
                    <SelectItem key={user.user_id} value={user.user_id}>
                      {user.name}
                    </SelectItem>
                  ))}
                </SelectGroup>
              </SelectContent>
            </Select>
            <Select
              value={userModelDraft.selectorKind}
              onValueChange={(value) =>
                setUserModelDraft((current) => ({
                  ...current,
                  selectorKind: value as UserModelDraft['selectorKind'],
                  selectorValue: '',
                }))
              }
            >
              <SelectTrigger>
                <SelectValue placeholder="Scope type" />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  <SelectItem value="model_id">Model id</SelectItem>
                  <SelectItem value="upstream_model">Upstream model</SelectItem>
                </SelectGroup>
              </SelectContent>
            </Select>
            {userModelDraft.selectorKind === 'model_id' ? (
              <Select
                value={userModelDraft.selectorValue}
                onValueChange={(value) =>
                  setUserModelDraft((current) => ({ ...current, selectorValue: value }))
                }
              >
                <SelectTrigger>
                  <SelectValue placeholder="Model" />
                </SelectTrigger>
                <SelectContent>
                  <SelectGroup>
                    {models.map((model) => (
                      <SelectItem key={model.model_id} value={model.model_id}>
                        {model.resolved_model_key}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                </SelectContent>
              </Select>
            ) : (
              <Input
                value={userModelDraft.selectorValue}
                onChange={(event) =>
                  setUserModelDraft((current) => ({
                    ...current,
                    selectorValue: event.currentTarget.value,
                  }))
                }
                placeholder="provider/model"
                autoComplete="off"
              />
            )}
            <Input
              value={userModelDraft.settings.amount_usd}
              onChange={(event) =>
                setUserModelDraft((current) => ({
                  ...current,
                  settings: { ...current.settings, amount_usd: event.currentTarget.value },
                }))
              }
              placeholder="100.0000"
              autoComplete="off"
            />
            <Select
              value={userModelDraft.settings.cadence}
              onValueChange={(value) =>
                setUserModelDraft((current) => ({
                  ...current,
                  settings: {
                    ...current.settings,
                    cadence: value as BudgetSettingsForm['cadence'],
                  },
                }))
              }
            >
              <SelectTrigger>
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
            <label className="flex min-h-10 items-center gap-2 text-sm text-[var(--color-text)]">
              <input
                type="checkbox"
                checked={userModelDraft.settings.hard_limit}
                onChange={(event) =>
                  setUserModelDraft((current) => ({
                    ...current,
                    settings: { ...current.settings, hard_limit: event.currentTarget.checked },
                  }))
                }
              />
              Hard
            </label>
            <Button type="submit" disabled={isPending}>
              Add
            </Button>
          </form>
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

      <Dialog
        open={dialogState.mode !== 'closed'}
        onOpenChange={(open) => (!open ? closeDialog() : null)}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Configure Budget</DialogTitle>
            <DialogDescription>
              Update cadence, limit amount, and hard-limit behavior for{' '}
              {openLabel ?? 'selected scope'}.
            </DialogDescription>
          </DialogHeader>
          <form className="flex flex-col gap-3" onSubmit={handleSave}>
            <BudgetSettingsFields form={form} setForm={setForm} />
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

function BudgetTable({
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
          <div className="grid grid-cols-[minmax(0,1fr)_170px_170px_220px_180px] bg-[color:var(--color-surface-muted)] text-[var(--color-text-soft)]">
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

function BudgetRow({ children }: { children: ReactNode }) {
  return (
    <div className="grid grid-cols-[minmax(0,1fr)_170px_170px_220px_180px] border-t border-[color:var(--color-border)]">
      {children}
    </div>
  )
}

function IdentityCell({ primary, secondary }: { primary: string; secondary: string }) {
  return (
    <div className="min-w-0 px-3 py-3">
      <p className="truncate text-sm font-semibold text-[var(--color-text)]">{primary}</p>
      <p className="truncate text-xs text-[var(--color-text-soft)]">{secondary}</p>
    </div>
  )
}

function BudgetCell({ budget }: { budget?: SpendBudgetUserView['budget'] }) {
  return (
    <div className="px-3 py-3">
      {budget ? (
        <Badge>{CURRENCY_FORMATTER.format(budget.amount_usd_10000 / 10_000)}</Badge>
      ) : (
        <span className="text-sm text-[var(--color-text-soft)]">Not set</span>
      )}
    </div>
  )
}

function MoneyCell({ amountUsd10000 }: { amountUsd10000: number }) {
  return (
    <span className="px-3 py-3 text-sm text-[var(--color-text-muted)]">
      {CURRENCY_FORMATTER.format(amountUsd10000 / 10_000)}
    </span>
  )
}

function TextCell({
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

function ActionCell({ children }: { children: ReactNode }) {
  return <div className="flex items-center gap-2 px-3 py-3">{children}</div>
}

function BudgetSettingsFields({
  form,
  setForm,
}: {
  form: BudgetSettingsForm
  setForm: Dispatch<SetStateAction<BudgetSettingsForm>>
}) {
  return (
    <>
      <div className="grid gap-1">
        <label className="text-xs font-semibold text-[var(--color-text-soft)]" htmlFor="budget-cadence">
          Cadence
        </label>
        <Select
          value={form.cadence}
          onValueChange={(value) =>
            setForm((current) => ({
              ...current,
              cadence: value as BudgetSettingsForm['cadence'],
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
        <label className="text-xs font-semibold text-[var(--color-text-soft)]" htmlFor="budget-amount">
          Amount (USD)
        </label>
        <Input
          id="budget-amount"
          value={form.amount_usd}
          onChange={(event) =>
            setForm((current) => ({ ...current, amount_usd: event.currentTarget.value }))
          }
          placeholder="100.0000"
          autoComplete="off"
        />
      </div>

      <div className="grid gap-1">
        <label className="text-xs font-semibold text-[var(--color-text-soft)]" htmlFor="budget-timezone">
          Timezone
        </label>
        <Input
          id="budget-timezone"
          value={form.timezone ?? 'UTC'}
          onChange={(event) =>
            setForm((current) => ({ ...current, timezone: event.currentTarget.value }))
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
            setForm((current) => ({ ...current, hard_limit: event.currentTarget.checked }))
          }
        />
        Enforce hard limit
      </label>
    </>
  )
}

function createInitialUserModelDraft(
  users: SpendBudgetUserView[],
  models: ModelView[],
): UserModelDraft {
  return {
    userId: users[0]?.user_id ?? '',
    selectorKind: 'model_id',
    selectorValue: models[0]?.model_id ?? '',
    settings: initialBudgetSettings,
  }
}

function settingsFromBudget(budget?: SpendBudgetUserView['budget']): BudgetSettingsForm {
  return {
    cadence: budget?.cadence ?? 'daily',
    amount_usd: budget?.amount_usd ?? '0.0000',
    hard_limit: budget?.hard_limit ?? true,
    timezone: budget?.timezone ?? 'UTC',
  }
}

function budgetPayload(scope: BudgetScopeRequest, settings: BudgetSettingsForm): UpsertBudgetInput {
  return {
    scope,
    cadence: settings.cadence,
    amount_usd: settings.amount_usd,
    hard_limit: settings.hard_limit,
    timezone: settings.timezone?.trim() || 'UTC',
  }
}

function scopeForDialog(dialogState: Exclude<BudgetDialogState, { mode: 'closed' }>) {
  if (dialogState.mode === 'user') {
    return { kind: 'user', user_id: dialogState.user.user_id }
  }
  if (dialogState.mode === 'service_account') {
    return {
      kind: 'service_account',
      service_account_id: dialogState.serviceAccount.service_account_id,
    }
  }
  return scopeForUserModelBudget(dialogState.budget)
}

function scopeForUserModelBudget(budget: SpendBudgetUserModelView): BudgetScopeRequest {
  if (budget.model_id) {
    return { kind: 'user_model', user_id: budget.user_id, model_id: budget.model_id }
  }
  return { kind: 'user_model', user_id: budget.user_id, upstream_model: budget.upstream_model }
}

function formatUserModelSelector(budget: SpendBudgetUserModelView) {
  if (budget.model_id) {
    return `model:${budget.model_id}`
  }
  return `upstream:${budget.upstream_model ?? ''}`
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

function formatOwnerKind(ownerKind: string) {
  if (ownerKind === 'service_account') {
    return 'service account'
  }

  return ownerKind
}
