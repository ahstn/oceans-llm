import { useState, useTransition, type Dispatch, type FormEvent, type SetStateAction } from 'react'
import { Link, useRouter } from '@tanstack/react-router'
import {
  ArrowLeft01Icon,
  ArrowRight01Icon,
  FilterHorizontalIcon,
  Search01Icon,
} from '@hugeicons/core-free-icons'
import { toast } from 'sonner'

import { AppIcon } from '@/components/icons/app-icon'
import {
  Timeline,
  TimelineContent,
  TimelineDate,
  TimelineHeader,
  TimelineIndicator,
  TimelineItem,
  TimelineSeparator,
  TimelineTitle,
} from '@/components/reui/timeline'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldGroup,
  FieldLabel,
} from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Progress } from '@/components/ui/progress'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { cn } from '@/lib/utils'
import { removeBudget, saveBudget } from '@/server/admin-data.functions'
import type {
  BudgetAlertHistoryItemView,
  BudgetScopeRequest,
  ModelView,
  SpendBudgetServiceAccountView,
  SpendBudgetUserModelView,
  SpendBudgetUserView,
} from '@/types/api'

import {
  alertBadgeVariant,
  budgetPayload,
  CURRENCY_FORMATTER,
  formatThreshold,
  getErrorMessage,
  initialBudgetSettings,
  LOW_USAGE_RATIO,
  PERCENT_FORMATTER,
  serviceAccountScope,
  settingsFromBudget,
  USAGE_STATUS_LABEL,
  USER_PAGE_SIZE,
  userModelScope,
  userScope,
  type BudgetSettingsForm,
  type BudgetStateFilter,
  type BudgetUsage,
  type UsageStatus,
  type UserBudgetList,
  type UserSortKey,
} from './-budget-lib'

// ---------------------------------------------------------------------------
// Candidate navigation
// ---------------------------------------------------------------------------

export const CANDIDATES = [
  { to: '/spend-controls', label: 'Current' },
  { to: '/spend-controls/ledger', label: 'A · Ledger' },
  { to: '/spend-controls/cards', label: 'B · Cards' },
  { to: '/spend-controls/workbench', label: 'C · Workbench' },
] as const

export type CandidatePath = (typeof CANDIDATES)[number]['to']

export function CandidateSwitcher({ current }: { current: CandidatePath }) {
  return (
    <nav aria-label="Design candidates" className="flex flex-wrap items-center gap-1">
      {CANDIDATES.map((candidate) => (
        <Button
          key={candidate.to}
          asChild
          size="sm"
          variant={candidate.to === current ? 'secondary' : 'ghost'}
        >
          <Link to={candidate.to}>{candidate.label}</Link>
        </Button>
      ))}
    </nav>
  )
}

// ---------------------------------------------------------------------------
// Budget editor: dialog state, form, and mutations shared by every candidate.
// ---------------------------------------------------------------------------

export type BudgetDialogTarget =
  | { mode: 'closed' }
  | { mode: 'user'; user: SpendBudgetUserView }
  | { mode: 'service_account'; serviceAccount: SpendBudgetServiceAccountView }
  | { mode: 'user_model'; budget: SpendBudgetUserModelView; label: string }

export interface BudgetEditor {
  target: BudgetDialogTarget
  form: BudgetSettingsForm
  setForm: Dispatch<SetStateAction<BudgetSettingsForm>>
  isPending: boolean
  openUser: (user: SpendBudgetUserView) => void
  openServiceAccount: (serviceAccount: SpendBudgetServiceAccountView) => void
  /** `label` is the display text for the dialog title, e.g. "Diego / model:claude-sonnet". */
  openUserModel: (budget: SpendBudgetUserModelView, label: string) => void
  close: () => void
  save: (event: FormEvent<HTMLFormElement>) => void
  createBudget: (scope: BudgetScopeRequest, settings: BudgetSettingsForm, message: string) => void
  remove: (scope: BudgetScopeRequest, message: string) => void
}

// One editor owns dialog state, the draft form, and every budget mutation so
// candidates only differ in layout.
// oxlint-disable-next-line eslint/max-lines-per-function
export function useBudgetEditor(onSaved?: () => void): BudgetEditor {
  const router = useRouter()
  const [target, setTarget] = useState<BudgetDialogTarget>({ mode: 'closed' })
  const [form, setForm] = useState<BudgetSettingsForm>(initialBudgetSettings)
  const [isPending, startTransition] = useTransition()

  function close() {
    setTarget({ mode: 'closed' })
    setForm(initialBudgetSettings)
  }

  function runMutation(mutation: () => Promise<void>, successMessage: string, after?: () => void) {
    startTransition(async () => {
      try {
        await mutation()
        toast.success(successMessage)
        await router.invalidate()
        after?.()
        onSaved?.()
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  return {
    target,
    form,
    setForm,
    isPending,
    close,
    openUser(user) {
      setTarget({ mode: 'user', user })
      setForm(settingsFromBudget(user.budget))
    },
    openServiceAccount(serviceAccount) {
      setTarget({ mode: 'service_account', serviceAccount })
      setForm(settingsFromBudget(serviceAccount.budget))
    },
    openUserModel(budget, label) {
      setTarget({ mode: 'user_model', budget, label })
      setForm(settingsFromBudget(budget.budget))
    },
    save(event) {
      event.preventDefault()
      if (target.mode === 'closed') return
      const scope =
        target.mode === 'user'
          ? userScope(target.user)
          : target.mode === 'service_account'
            ? serviceAccountScope(target.serviceAccount)
            : userModelScope(target.budget)
      runMutation(
        async () => {
          await saveBudget({ data: budgetPayload(scope, form) })
        },
        'Budget updated',
        close,
      )
    },
    createBudget(scope, settings, message) {
      runMutation(async () => {
        await saveBudget({ data: budgetPayload(scope, settings) })
      }, message)
    },
    remove(scope, message) {
      runMutation(async () => {
        await removeBudget({ data: { scope } })
      }, message)
    },
  }
}

export function budgetTargetLabel(target: BudgetDialogTarget) {
  switch (target.mode) {
    case 'user':
      return target.user.name
    case 'service_account':
      return target.serviceAccount.service_account_name
    case 'user_model':
      return target.label
    case 'closed':
      return null
  }
}

export function BudgetDialog({ editor }: { editor: BudgetEditor }) {
  const label = budgetTargetLabel(editor.target)
  return (
    <Dialog
      open={editor.target.mode !== 'closed'}
      onOpenChange={(open) => (open ? null : editor.close())}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Configure budget</DialogTitle>
          <DialogDescription>
            Set the cadence, limit, and hard-limit behavior for {label ?? 'the selected scope'}.
          </DialogDescription>
        </DialogHeader>
        <form className="flex flex-col gap-5" onSubmit={editor.save}>
          <BudgetSettingsFields form={editor.form} setForm={editor.setForm} idPrefix="dialog" />
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={editor.close}>
              Cancel
            </Button>
            <Button type="submit" disabled={editor.isPending}>
              {editor.isPending ? 'Saving...' : 'Save budget'}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

export function BudgetSettingsFields({
  form,
  setForm,
  idPrefix,
  compact = false,
}: {
  form: BudgetSettingsForm
  setForm: Dispatch<SetStateAction<BudgetSettingsForm>>
  idPrefix: string
  compact?: boolean
}) {
  return (
    <FieldGroup className={cn(compact && 'gap-3')}>
      <div className={cn('grid gap-3', compact ? 'grid-cols-2' : 'sm:grid-cols-2')}>
        <Field>
          <FieldLabel htmlFor={`${idPrefix}-amount`}>Amount (USD)</FieldLabel>
          <Input
            id={`${idPrefix}-amount`}
            inputMode="decimal"
            value={form.amount_usd}
            onChange={(event) => {
              const amount_usd = event.currentTarget.value
              setForm((current) => ({ ...current, amount_usd }))
            }}
            placeholder="100.0000"
            autoComplete="off"
          />
        </Field>
        <Field>
          <FieldLabel htmlFor={`${idPrefix}-cadence`}>Cadence</FieldLabel>
          <Select
            value={form.cadence}
            onValueChange={(value) =>
              setForm((current) => ({
                ...current,
                cadence: value as BudgetSettingsForm['cadence'],
              }))
            }
          >
            <SelectTrigger id={`${idPrefix}-cadence`} className="w-full">
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
        </Field>
      </div>
      <Field>
        <FieldLabel htmlFor={`${idPrefix}-timezone`}>Timezone</FieldLabel>
        <Input
          id={`${idPrefix}-timezone`}
          value={form.timezone ?? 'UTC'}
          onChange={(event) => {
            const timezone = event.currentTarget.value
            setForm((current) => ({ ...current, timezone }))
          }}
          placeholder="UTC"
          autoComplete="off"
        />
        {compact ? null : (
          <FieldDescription>Controls when the budget window resets.</FieldDescription>
        )}
      </Field>
      <Field orientation="horizontal">
        <FieldContent>
          <FieldLabel htmlFor={`${idPrefix}-hard-limit`}>Enforce hard limit</FieldLabel>
          {compact ? null : (
            <FieldDescription>Block requests once the budget is exhausted.</FieldDescription>
          )}
        </FieldContent>
        <Switch
          id={`${idPrefix}-hard-limit`}
          checked={form.hard_limit}
          onCheckedChange={(checked) => setForm((current) => ({ ...current, hard_limit: checked }))}
        />
      </Field>
    </FieldGroup>
  )
}

// ---------------------------------------------------------------------------
// User model budget creation form.
// ---------------------------------------------------------------------------

export type UserModelDraft = {
  userId: string
  selectorKind: 'model_id' | 'upstream_model'
  selectorValue: string
  settings: BudgetSettingsForm
}

export function initialUserModelDraft(
  users: SpendBudgetUserView[],
  models: ModelView[],
  userId?: string,
): UserModelDraft {
  return {
    userId: userId ?? users[0]?.user_id ?? '',
    selectorKind: 'model_id',
    selectorValue: models[0]?.model_id ?? '',
    settings: initialBudgetSettings,
  }
}

// A linear controlled form; the selector kind swaps one field and the rest is shared.
// oxlint-disable-next-line eslint/max-lines-per-function
export function UserModelBudgetForm({
  users,
  models,
  editor,
  lockedUserId,
  onCreated,
}: {
  users: SpendBudgetUserView[]
  models: ModelView[]
  editor: BudgetEditor
  /** When set, the user picker is hidden and the draft is pinned to this user. */
  lockedUserId?: string
  onCreated?: () => void
}) {
  const [draft, setDraft] = useState<UserModelDraft>(() =>
    initialUserModelDraft(users, models, lockedUserId),
  )
  const userId = lockedUserId ?? draft.userId

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const selectorValue = draft.selectorValue.trim()
    if (!userId || !selectorValue) {
      toast.error('Select a user and model scope before saving')
      return
    }
    const scope: BudgetScopeRequest =
      draft.selectorKind === 'model_id'
        ? { kind: 'user_model', user_id: userId, model_id: selectorValue }
        : {
            kind: 'user_model',
            user_id: userId,
            upstream_model: selectorValue,
          }
    editor.createBudget(scope, draft.settings, 'User model budget created')
    setDraft(initialUserModelDraft(users, models, lockedUserId))
    onCreated?.()
  }

  return (
    <form className="flex flex-col gap-4" onSubmit={submit}>
      <FieldGroup className="gap-3">
        {lockedUserId ? null : (
          <Field>
            <FieldLabel htmlFor="user-model-user">User</FieldLabel>
            <Select
              value={draft.userId}
              onValueChange={(value) => setDraft((current) => ({ ...current, userId: value }))}
            >
              <SelectTrigger id="user-model-user" className="w-full">
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
          </Field>
        )}
        <div className="grid gap-3 sm:grid-cols-[160px_minmax(0,1fr)]">
          <Field>
            <FieldLabel htmlFor="user-model-kind">Scope</FieldLabel>
            <Select
              value={draft.selectorKind}
              onValueChange={(value) =>
                setDraft((current) => ({
                  ...current,
                  selectorKind: value as UserModelDraft['selectorKind'],
                  selectorValue: value === 'model_id' ? (models[0]?.model_id ?? '') : '',
                }))
              }
            >
              <SelectTrigger id="user-model-kind" className="w-full">
                <SelectValue placeholder="Scope type" />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  <SelectItem value="model_id">Model id</SelectItem>
                  <SelectItem value="upstream_model">Upstream model</SelectItem>
                </SelectGroup>
              </SelectContent>
            </Select>
          </Field>
          <Field>
            <FieldLabel htmlFor="user-model-value">
              {draft.selectorKind === 'model_id' ? 'Model' : 'Upstream model'}
            </FieldLabel>
            {draft.selectorKind === 'model_id' ? (
              <Select
                value={draft.selectorValue}
                onValueChange={(value) =>
                  setDraft((current) => ({ ...current, selectorValue: value }))
                }
              >
                <SelectTrigger id="user-model-value" className="w-full">
                  <SelectValue placeholder="Model" />
                </SelectTrigger>
                <SelectContent>
                  <SelectGroup>
                    {models.map((model) => (
                      <SelectItem key={model.model_id} value={model.model_id}>
                        {model.id}
                        {model.resolved_model_key !== model.id ? (
                          <span className="text-muted-foreground">
                            {' '}
                            · {model.resolved_model_key}
                          </span>
                        ) : null}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                </SelectContent>
              </Select>
            ) : (
              <Input
                id="user-model-value"
                value={draft.selectorValue}
                onChange={(event) => {
                  const selectorValue = event.currentTarget.value
                  setDraft((current) => ({ ...current, selectorValue }))
                }}
                placeholder="provider/model"
                autoComplete="off"
              />
            )}
          </Field>
        </div>
      </FieldGroup>
      <BudgetSettingsFields
        form={draft.settings}
        setForm={(update) =>
          setDraft((current) => ({
            ...current,
            settings: typeof update === 'function' ? update(current.settings) : update,
          }))
        }
        idPrefix="user-model"
        compact
      />
      <div className="flex justify-end">
        <Button type="submit" disabled={editor.isPending}>
          Add model budget
        </Button>
      </div>
    </form>
  )
}

// ---------------------------------------------------------------------------
// User list toolbar: search, filters popover, sort, and pager.
// ---------------------------------------------------------------------------

const SORT_LABEL: Record<UserSortKey, string> = {
  spend: 'Spend, high to low',
  usage: 'Budget used, high to low',
  name: 'Name, A to Z',
}

const BUDGET_STATE_LABEL: Record<BudgetStateFilter, string> = {
  all: 'All users',
  budgeted: 'With budget',
  unbudgeted: 'Without budget',
}

// oxlint-disable-next-line eslint/max-lines-per-function
export function UserListToolbar({ list, className }: { list: UserBudgetList; className?: string }) {
  return (
    <div className={cn('flex flex-wrap items-center gap-2', className)}>
      <InputGroup className="w-full sm:max-w-xs">
        <InputGroupAddon>
          <AppIcon icon={Search01Icon} size={14} stroke={1.5} />
        </InputGroupAddon>
        <InputGroupInput
          aria-label="Search users"
          placeholder="Search name, email, or team"
          value={list.filters.query}
          onChange={(event) => list.setFilters({ query: event.currentTarget.value })}
        />
      </InputGroup>
      <Popover>
        <PopoverTrigger asChild>
          <Button type="button" variant="outline" size="sm" className="gap-2">
            <AppIcon icon={FilterHorizontalIcon} size={14} stroke={1.5} data-icon="inline-start" />
            Filters
            {list.activeFilterCount > 0 ? (
              <Badge variant="secondary">{list.activeFilterCount}</Badge>
            ) : null}
          </Button>
        </PopoverTrigger>
        <PopoverContent align="start" className="w-72 gap-4 p-3">
          <div className="flex flex-col gap-1">
            <h2 className="text-sm font-medium">Filters</h2>
            <p className="text-muted-foreground text-xs">
              Low-usage users are hidden by default so the list stays focused on active spend.
            </p>
          </div>
          <FieldGroup className="gap-3">
            <Field orientation="horizontal">
              <Checkbox
                id="filter-hide-low"
                checked={list.filters.hideLowUsage}
                onCheckedChange={(checked) => list.setFilters({ hideLowUsage: checked === true })}
              />
              <FieldContent>
                <FieldLabel htmlFor="filter-hide-low">Hide idle users</FieldLabel>
                <FieldDescription>
                  Users under {PERCENT_FORMATTER.format(LOW_USAGE_RATIO)} of their budget, or with
                  no budget and no spend.
                </FieldDescription>
              </FieldContent>
            </Field>
            <Field>
              <FieldLabel>Budget</FieldLabel>
              <ToggleGroup
                type="single"
                variant="outline"
                size="sm"
                spacing={0}
                value={list.filters.budgetState}
                onValueChange={(value) => {
                  if (value)
                    list.setFilters({
                      budgetState: value as BudgetStateFilter,
                    })
                }}
                aria-label="Budget state"
              >
                {(Object.keys(BUDGET_STATE_LABEL) as BudgetStateFilter[]).map((state) => (
                  <ToggleGroupItem key={state} value={state}>
                    {BUDGET_STATE_LABEL[state]}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>
            </Field>
          </FieldGroup>
          <Button type="button" variant="ghost" size="sm" onClick={list.clearFilters}>
            Clear filters
          </Button>
        </PopoverContent>
      </Popover>
      <Select
        value={list.filters.sort}
        onValueChange={(value) => list.setFilters({ sort: value as UserSortKey })}
      >
        <SelectTrigger size="sm" className="w-[210px]" aria-label="Sort users">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectGroup>
            {(Object.keys(SORT_LABEL) as UserSortKey[]).map((key) => (
              <SelectItem key={key} value={key}>
                {SORT_LABEL[key]}
              </SelectItem>
            ))}
          </SelectGroup>
        </SelectContent>
      </Select>
    </div>
  )
}

export function ListPager({ list, className }: { list: UserBudgetList; className?: string }) {
  const from = list.visibleCount === 0 ? 0 : (list.page - 1) * USER_PAGE_SIZE + 1
  const to = Math.min(list.page * USER_PAGE_SIZE, list.visibleCount)
  return (
    <div className={cn('flex flex-wrap items-center justify-between gap-3', className)}>
      <p className="text-muted-foreground text-sm">
        Showing {from}–{to} of {list.visibleCount} users
        {list.hiddenCount > 0 ? ` · ${list.hiddenCount} hidden by filters` : ''}
      </p>
      <div className="flex items-center gap-2">
        <span className="text-muted-foreground text-sm">
          Page {list.page} of {list.totalPages}
        </span>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => list.setPage(list.page - 1)}
          disabled={list.page <= 1}
        >
          <AppIcon icon={ArrowLeft01Icon} size={14} stroke={1.5} data-icon="inline-start" />
          Previous
        </Button>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => list.setPage(list.page + 1)}
          disabled={list.page >= list.totalPages}
        >
          Next
          <AppIcon icon={ArrowRight01Icon} size={14} stroke={1.5} data-icon="inline-end" />
        </Button>
      </div>
    </div>
  )
}

// ---------------------------------------------------------------------------
// Usage presentation.
// ---------------------------------------------------------------------------

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

const STATUS_BAR_CLASS: Record<UsageStatus, string> = {
  no_budget: '',
  low: '[&>[data-slot=progress-indicator]]:bg-muted-foreground/60',
  on_track: '',
  warning: '[&>[data-slot=progress-indicator]]:bg-[var(--color-warning)]',
  over: '[&>[data-slot=progress-indicator]]:bg-destructive',
}

export function UsageStatusBadge({ status }: { status: UsageStatus }) {
  return <Badge variant={STATUS_BADGE_VARIANT[status]}>{USAGE_STATUS_LABEL[status]}</Badge>
}

export function UsageBar({
  usage,
  showAmounts = true,
  className,
}: {
  usage: BudgetUsage
  showAmounts?: boolean
  className?: string
}) {
  if (usage.ratio === null) {
    return (
      <div className={cn('flex min-w-0 flex-col gap-1', className)}>
        {showAmounts ? (
          <span className="text-sm font-medium">{CURRENCY_FORMATTER.format(usage.spendUsd)}</span>
        ) : null}
        <span className="text-muted-foreground text-xs">No budget set</span>
      </div>
    )
  }
  return (
    <div className={cn('flex min-w-0 flex-col gap-1.5', className)}>
      {showAmounts ? (
        <div className="flex items-baseline justify-between gap-2 text-sm">
          <span className="font-medium">{CURRENCY_FORMATTER.format(usage.spendUsd)}</span>
          <span className="text-muted-foreground text-xs">
            of {CURRENCY_FORMATTER.format(usage.budgetUsd ?? 0)}
          </span>
        </div>
      ) : null}
      <Progress
        value={Math.min(100, usage.ratio * 100)}
        aria-label={`${PERCENT_FORMATTER.format(usage.ratio)} of budget used`}
        className={cn('h-1.5', STATUS_BAR_CLASS[usage.status])}
      />
    </div>
  )
}

// ---------------------------------------------------------------------------
// Alert history timeline (ReUI Timeline).
// ---------------------------------------------------------------------------

const ALERT_INDICATOR_CLASS: Record<string, string> = {
  sent: 'border-[var(--color-success)] bg-[var(--color-success-soft)]',
  pending: 'border-[var(--color-warning)] bg-[var(--color-warning-soft)]',
  failed: 'border-destructive bg-destructive/10',
}

export function AlertTimeline({
  items,
  emptyMessage = 'No budget alerts have been recorded yet.',
}: {
  items: BudgetAlertHistoryItemView[]
  emptyMessage?: string
}) {
  if (items.length === 0) {
    return <p className="text-muted-foreground py-4 text-sm">{emptyMessage}</p>
  }
  return (
    <Timeline value={0} className="pl-6">
      {items.map((alert, index) => (
        <TimelineItem key={alert.budget_alert_id} step={index + 1}>
          <TimelineHeader>
            <TimelineSeparator />
            <TimelineDate dateTime={alert.created_at}>
              {new Date(alert.created_at).toLocaleString()}
            </TimelineDate>
            <TimelineTitle className="flex flex-wrap items-center gap-2">
              <span className="truncate">{alert.owner_name}</span>
              <Badge variant="outline">
                {alert.owner_kind === 'service_account' ? 'service account' : alert.owner_kind}
              </Badge>
              <Badge variant={alertBadgeVariant(alert)}>{alert.delivery_status}</Badge>
            </TimelineTitle>
            <TimelineIndicator className={ALERT_INDICATOR_CLASS[alert.delivery_status] ?? ''} />
          </TimelineHeader>
          <TimelineContent>
            Crossed {formatThreshold(alert)} of the {alert.cadence} budget with{' '}
            {CURRENCY_FORMATTER.format(alert.remaining_budget_usd_10000 / 10_000)} remaining.
            Notified {alert.recipient_summary}.
            {alert.failure_reason ? (
              <span className="text-destructive mt-1 block text-xs">{alert.failure_reason}</span>
            ) : null}
          </TimelineContent>
        </TimelineItem>
      ))}
    </Timeline>
  )
}
