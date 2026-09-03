import { useState } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import {
  AlertDiamondIcon,
  Coins01Icon,
  Delete02Icon,
  Mail01Icon,
  RoboticIcon,
  Settings02Icon,
  UserIcon,
  Wallet01Icon,
} from '@hugeicons/core-free-icons'

import { AppIcon } from '@/components/icons/app-icon'
import { PageHeader } from '@/components/layout/page-header'
import { IconTile } from '@/components/reui/icon-tile'
import { Button } from '@/components/ui/button'
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from '@/components/ui/empty'
import { GeneratedAvatar } from '@/components/ui/generated-avatar'
import { Separator } from '@/components/ui/separator'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { cn } from '@/lib/utils'
import type {
  BudgetAlertHistoryItemView,
  ModelView,
  SpendBudgetServiceAccountView,
  SpendBudgetUserModelView,
  SpendBudgetUserView,
} from '@/types/api'

import {
  AlertTimeline,
  BudgetDialog,
  CandidateSwitcher,
  ListPager,
  UsageBar,
  UsageStatusBadge,
  UserListToolbar,
  UserModelBudgetForm,
  useBudgetEditor,
  type BudgetEditor,
} from './-budget-components'
import {
  budgetUsage,
  CURRENCY_FORMATTER,
  formatCadence,
  formatUserModelSelector,
  loadSpendControls,
  serviceAccountScope,
  userModelScope,
  userScope,
  useUserBudgetList,
  type BudgetUsage,
  type SpendControlsLoaderData,
  type UserBudgetList,
} from './-budget-lib'

export const Route = createFileRoute('/spend-controls_/workbench')({
  loader: loadSpendControls,
  component: WorkbenchPage,
})

// Master–detail: the selected user drives three detail cards, so the page owns
// selection state and hands each slice of loader data to a focused card.
// oxlint-disable-next-line eslint/max-lines-per-function
export function WorkbenchPage() {
  const { budgets, alerts, models } = Route.useLoaderData() as SpendControlsLoaderData
  const users = budgets.data.users
  const list = useUserBudgetList(users)
  const editor = useBudgetEditor()
  const [selectedUserId, setSelectedUserId] = useState<string | null>(null)

  // Derived rather than synced: a stale selection (filtered out or removed by a
  // refetch) falls back to the first visible row without an effect round-trip.
  const selectedUser =
    users.find((user) => user.user_id === selectedUserId) ?? list.pageRows[0]?.user ?? null

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      <PageHeader
        section="Budget & Spending"
        title="Spend controls"
        description="Pick a user to review their budget, model limits, and alerts side by side. Service account budgets are managed below."
        actions={<CandidateSwitcher current="/spend-controls/workbench" />}
      />

      <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.2fr)]">
        <UserListCard
          list={list}
          selectedUserId={selectedUser?.user_id ?? null}
          onSelect={setSelectedUserId}
        />

        <div className="flex min-w-0 flex-col gap-6">
          {selectedUser ? (
            <>
              <UserBudgetCard user={selectedUser} editor={editor} />
              <UserModelBudgetsCard
                key={selectedUser.user_id}
                user={selectedUser}
                budgets={budgets.data.user_model_budgets}
                users={users}
                models={models.data.items}
                editor={editor}
              />
              <UserAlertsCard user={selectedUser} alerts={alerts.data.items} />
            </>
          ) : (
            <Card>
              <CardContent>
                <Empty>
                  <EmptyHeader>
                    <EmptyMedia variant="icon">
                      <AppIcon icon={UserIcon} size={16} stroke={1.5} />
                    </EmptyMedia>
                    <EmptyTitle>Select a user</EmptyTitle>
                    <EmptyDescription>
                      Choose a user from the list to see their budget, model limits, and alerts.
                    </EmptyDescription>
                  </EmptyHeader>
                </Empty>
              </CardContent>
            </Card>
          )}
        </div>
      </div>

      <ServiceAccountsCard serviceAccounts={budgets.data.service_accounts} editor={editor} />

      <BudgetDialog editor={editor} />
    </div>
  )
}

// ---------------------------------------------------------------------------
// Left column: selectable user list.
// ---------------------------------------------------------------------------

function UserListCard({
  list,
  selectedUserId,
  onSelect,
}: {
  list: UserBudgetList
  selectedUserId: string | null
  onSelect: (userId: string) => void
}) {
  return (
    <Card className="min-w-0 self-start">
      <CardHeader>
        <CardTitle>Users</CardTitle>
        <CardDescription>
          Sorted by spend. Idle users are hidden until you widen the filters.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex min-w-0 flex-col gap-4">
        <UserListToolbar list={list} />

        {list.pageRows.length === 0 ? (
          <Empty className="border">
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <AppIcon icon={UserIcon} size={16} stroke={1.5} />
              </EmptyMedia>
              <EmptyTitle>No users match</EmptyTitle>
              <EmptyDescription>
                {list.activeFilterCount > 0
                  ? 'Adjust the search or filters to see more users.'
                  : 'No users have recorded spend in this window.'}
              </EmptyDescription>
            </EmptyHeader>
            {list.activeFilterCount > 0 ? (
              <Button type="button" variant="outline" size="sm" onClick={list.clearFilters}>
                Clear filters
              </Button>
            ) : null}
          </Empty>
        ) : (
          <div className="flex flex-col divide-y rounded-md border">
            {list.pageRows.map(({ user, usage }) => (
              <UserListRow
                key={user.user_id}
                user={user}
                usage={usage}
                selected={user.user_id === selectedUserId}
                onSelect={() => onSelect(user.user_id)}
              />
            ))}
          </div>
        )}

        <ListPager list={list} />
      </CardContent>
    </Card>
  )
}

function UserListRow({
  user,
  usage,
  selected,
  onSelect,
}: {
  user: SpendBudgetUserView
  usage: BudgetUsage
  selected: boolean
  onSelect: () => void
}) {
  return (
    <button
      type="button"
      aria-pressed={selected}
      onClick={onSelect}
      className={cn(
        'hover:bg-muted/60 focus-visible:ring-ring/50 flex w-full items-center gap-3 px-3 py-2.5 text-left outline-none focus-visible:ring-[3px]',
        selected && 'bg-muted',
      )}
    >
      <GeneratedAvatar kind="user" name={user.name} size={32} />
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="truncate text-sm font-medium">{user.name}</span>
        <span className="text-muted-foreground truncate text-xs">{user.email}</span>
      </div>
      <UsageBar usage={usage} showAmounts={false} className="w-28 shrink-0" />
      <span className="w-20 shrink-0 text-right text-sm font-medium tabular-nums">
        {CURRENCY_FORMATTER.format(usage.spendUsd)}
      </span>
      <UsageStatusBadge status={usage.status} />
    </button>
  )
}

// ---------------------------------------------------------------------------
// Right column: everything about the selected user.
// ---------------------------------------------------------------------------

function UserBudgetCard({ user, editor }: { user: SpendBudgetUserView; editor: BudgetEditor }) {
  const usage = budgetUsage(user)
  const budget = user.budget
  const limit = budget
    ? budget.hard_limit
      ? { value: 'Hard', hint: 'Requests blocked at 100%' }
      : { value: 'Soft', hint: 'Alerts only' }
    : { value: '—', hint: 'Unlimited' }
  return (
    <Card className="min-w-0">
      <CardHeader className="flex flex-row items-center gap-3">
        <GeneratedAvatar kind="user" name={user.name} size={40} />
        <div className="flex min-w-0 flex-col">
          <CardTitle className="truncate">{user.name}</CardTitle>
          <CardDescription className="truncate">
            {user.email}
            {user.team_name ? ` · ${user.team_name}` : ''}
          </CardDescription>
        </div>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <UsageBar usage={usage} className="[&_[data-slot=progress]]:h-2" />

        <div className="grid gap-3 sm:grid-cols-3">
          <Metric
            icon={Wallet01Icon}
            label="Budget"
            value={usage.budgetUsd === null ? '—' : CURRENCY_FORMATTER.format(usage.budgetUsd)}
            hint={budget ? formatCadence(budget.cadence) : 'No budget set'}
          />
          <Metric
            icon={Coins01Icon}
            label="Remaining"
            value={
              usage.remainingUsd === null ? '—' : CURRENCY_FORMATTER.format(usage.remainingUsd)
            }
            hint={`${CURRENCY_FORMATTER.format(usage.spendUsd)} spent this window`}
          />
          <Metric icon={AlertDiamondIcon} label="Limit" value={limit.value} hint={limit.hint} />
        </div>

        <p
          className={cn(
            'flex items-center gap-1.5 text-xs',
            user.alert_email_ready ? 'text-muted-foreground' : 'text-destructive',
          )}
        >
          <AppIcon icon={Mail01Icon} size={14} stroke={1.5} />
          {user.alert_recipient_summary}
        </p>
      </CardContent>
      <CardFooter className="gap-2">
        <Button type="button" onClick={() => editor.openUser(user)}>
          <AppIcon icon={Settings02Icon} size={14} stroke={1.5} data-icon="inline-start" />
          Configure
        </Button>
        {budget ? (
          <Button
            type="button"
            variant="ghost"
            disabled={editor.isPending}
            onClick={() => editor.remove(userScope(user), 'User budget removed')}
          >
            <AppIcon icon={Delete02Icon} size={14} stroke={1.5} data-icon="inline-start" />
            Remove
          </Button>
        ) : null}
      </CardFooter>
    </Card>
  )
}

function Metric({
  icon,
  label,
  value,
  hint,
}: {
  icon: typeof Wallet01Icon
  label: string
  value: string
  hint: string
}) {
  return (
    <div className="flex items-start gap-3">
      <IconTile variant="frame" size="sm">
        <AppIcon icon={icon} size={16} stroke={1.5} />
      </IconTile>
      <div className="flex min-w-0 flex-col">
        <span className="text-muted-foreground text-xs">{label}</span>
        <span className="text-sm font-semibold tabular-nums">{value}</span>
        <span className="text-muted-foreground truncate text-xs">{hint}</span>
      </div>
    </div>
  )
}

function UserModelBudgetsCard({
  user,
  budgets,
  users,
  models,
  editor,
}: {
  user: SpendBudgetUserView
  budgets: SpendBudgetUserModelView[]
  users: SpendBudgetUserView[]
  models: ModelView[]
  editor: BudgetEditor
}) {
  const own = budgets.filter((budget) => budget.user_id === user.user_id)
  return (
    <Card className="min-w-0">
      <CardHeader>
        <CardTitle>Model budgets for this user</CardTitle>
        <CardDescription>
          Per-model or per-upstream caps that apply on top of the user budget.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        {own.length === 0 ? (
          <p className="text-muted-foreground text-sm">No model budgets for {user.name} yet.</p>
        ) : (
          <div className="flex flex-col divide-y rounded-md border">
            {own.map((budget) => {
              const selector = formatUserModelSelector(budget, models)
              return (
                <div key={budget.budget_id} className="flex items-center gap-3 px-3 py-2.5">
                  <code className="min-w-0 flex-1 truncate font-mono text-xs">{selector}</code>
                  <UsageBar usage={budgetUsage(budget)} className="w-40 shrink-0" />
                  <div className="flex shrink-0 items-center gap-1">
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      onClick={() => editor.openUserModel(budget, `${user.name} / ${selector}`)}
                    >
                      Configure
                    </Button>
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      disabled={editor.isPending}
                      onClick={() =>
                        editor.remove(userModelScope(budget), 'User model budget removed')
                      }
                    >
                      Remove
                    </Button>
                  </div>
                </div>
              )
            })}
          </div>
        )}

        <Separator />

        <UserModelBudgetForm
          users={users}
          models={models}
          editor={editor}
          lockedUserId={user.user_id}
        />
      </CardContent>
    </Card>
  )
}

function UserAlertsCard({
  user,
  alerts,
}: {
  user: SpendBudgetUserView
  alerts: BudgetAlertHistoryItemView[]
}) {
  const own = alerts.filter(
    (alert) => alert.owner_kind === 'user' && alert.owner_id === user.user_id,
  )
  return (
    <Card className="min-w-0">
      <CardHeader>
        <CardTitle>Recent alerts</CardTitle>
        <CardDescription>Threshold notifications sent for {user.name}.</CardDescription>
      </CardHeader>
      <CardContent>
        <AlertTimeline items={own} emptyMessage="No alerts for this user yet." />
      </CardContent>
    </Card>
  )
}

// ---------------------------------------------------------------------------
// Service accounts.
// ---------------------------------------------------------------------------

function ServiceAccountsCard({
  serviceAccounts,
  editor,
}: {
  serviceAccounts: SpendBudgetServiceAccountView[]
  editor: BudgetEditor
}) {
  return (
    <Card className="min-w-0">
      <CardHeader>
        <CardTitle>Service account budgets</CardTitle>
        <CardDescription>
          Caps for automated callers. Alerts go to the owning team unless recipients are missing.
        </CardDescription>
      </CardHeader>
      <CardContent>
        {serviceAccounts.length === 0 ? (
          <Empty className="border">
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <AppIcon icon={RoboticIcon} size={16} stroke={1.5} />
              </EmptyMedia>
              <EmptyTitle>No service accounts</EmptyTitle>
              <EmptyDescription>
                Service accounts appear here once a team creates one.
              </EmptyDescription>
            </EmptyHeader>
          </Empty>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Service account</TableHead>
                <TableHead>Budget</TableHead>
                <TableHead className="w-56">Usage</TableHead>
                <TableHead>Alert recipients</TableHead>
                <TableHead className="w-48 text-right">Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {serviceAccounts.map((sa) => (
                <ServiceAccountRow key={sa.service_account_id} sa={sa} editor={editor} />
              ))}
            </TableBody>
          </Table>
        )}
      </CardContent>
    </Card>
  )
}

function ServiceAccountRow({
  sa,
  editor,
}: {
  sa: SpendBudgetServiceAccountView
  editor: BudgetEditor
}) {
  const usage = budgetUsage(sa)
  return (
    <TableRow>
      <TableCell>
        <div className="flex min-w-0 flex-col">
          <span className="truncate font-medium">{sa.service_account_name}</span>
          <span className="text-muted-foreground truncate text-xs">
            {sa.service_account_key} / {sa.team_name}
          </span>
        </div>
      </TableCell>
      <TableCell>
        {sa.budget ? (
          <div className="flex flex-col">
            <span className="tabular-nums">
              {CURRENCY_FORMATTER.format(sa.budget.amount_usd_10000 / 10_000)}
            </span>
            <span className="text-muted-foreground text-xs">
              {formatCadence(sa.budget.cadence)} · {sa.budget.hard_limit ? 'Hard' : 'Soft'} limit
            </span>
          </div>
        ) : (
          <span className="text-muted-foreground">No budget</span>
        )}
      </TableCell>
      <TableCell>
        <UsageBar usage={usage} />
      </TableCell>
      <TableCell className={cn('text-xs', !sa.alert_email_ready && 'text-destructive')}>
        <span className="flex items-center gap-1.5">
          <AppIcon icon={Mail01Icon} size={14} stroke={1.5} />
          {sa.alert_recipient_summary}
        </span>
      </TableCell>
      <TableCell>
        <div className="flex items-center justify-end gap-1">
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => editor.openServiceAccount(sa)}
          >
            Configure
          </Button>
          {sa.budget ? (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              disabled={editor.isPending}
              onClick={() =>
                editor.remove(serviceAccountScope(sa), 'Service account budget removed')
              }
            >
              Remove
            </Button>
          ) : null}
        </div>
      </TableCell>
    </TableRow>
  )
}
