import { useMemo, useState } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { Add01Icon, RoboticIcon, Settings02Icon, UserGroupIcon } from '@hugeicons/core-free-icons'

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
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from '@/components/ui/empty'
import { GeneratedAvatar } from '@/components/ui/generated-avatar'
import type {
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
  useBudgetEditor,
  UserListToolbar,
  UserModelBudgetForm,
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
  type SpendControlsLoaderData,
  type UserBudgetList,
  type UserBudgetRow,
} from './-budget-lib'

export const Route = createFileRoute('/spend-controls_/cards')({
  loader: loadSpendControls,
  component: CardsPage,
})

// The page composes every budget surface; each card body lives in its own component below.
// oxlint-disable-next-line eslint/max-lines-per-function
export function CardsPage() {
  const { budgets, alerts, models } = Route.useLoaderData() as SpendControlsLoaderData
  const {
    users,
    service_accounts: serviceAccounts,
    user_model_budgets: userModelBudgets,
  } = budgets.data
  const editor = useBudgetEditor()
  const list = useUserBudgetList(users)
  const usersById = useMemo(() => new Map(users.map((user) => [user.user_id, user])), [users])

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      <PageHeader
        section="Budget & Spending"
        title="Spend controls"
        description="Scan who is spending against their limits, then tune budgets for users, service accounts, and model-specific scopes."
        actions={
          <div className="flex items-center gap-2">
            <CandidateSwitcher current="/spend-controls/cards" />
            <AddModelBudgetButton users={users} models={models.data.items} editor={editor} />
          </div>
        }
      />

      <Card>
        <CardHeader>
          <CardTitle>User budgets</CardTitle>
          <CardDescription>
            One card per user, sorted by current spend. Low-usage users are hidden until you change
            the filters.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <UserListToolbar list={list} />
          <UserCardGrid list={list} editor={editor} />
          <ListPager list={list} />
        </CardContent>
      </Card>

      <div className="grid gap-6 lg:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle>Service accounts</CardTitle>
            <CardDescription>
              Active service-account keys require an active service-account budget.
            </CardDescription>
          </CardHeader>
          <CardContent>
            {serviceAccounts.length === 0 ? (
              <p className="text-muted-foreground py-4 text-sm">
                No service accounts are available.
              </p>
            ) : (
              <ul className="divide-border flex flex-col divide-y">
                {serviceAccounts.map((serviceAccount) => (
                  <ServiceAccountRow
                    key={serviceAccount.service_account_id}
                    serviceAccount={serviceAccount}
                    editor={editor}
                  />
                ))}
              </ul>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Model budgets</CardTitle>
            <CardDescription>
              Model-specific budgets are evaluated before the user's general budget.
            </CardDescription>
          </CardHeader>
          <CardContent>
            {userModelBudgets.length === 0 ? (
              <p className="text-muted-foreground py-4 text-sm">
                No user model budgets are configured.
              </p>
            ) : (
              <ul className="divide-border flex flex-col divide-y">
                {userModelBudgets.map((budget) => (
                  <UserModelRow
                    key={budget.scope_key}
                    budget={budget}
                    ownerName={usersById.get(budget.user_id)?.name ?? budget.user_id}
                    models={models.data.items}
                    editor={editor}
                  />
                ))}
              </ul>
            )}
          </CardContent>
        </Card>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Alert history</CardTitle>
          <CardDescription>Recent threshold crossings and how they were delivered.</CardDescription>
        </CardHeader>
        <CardContent>
          <AlertTimeline items={alerts.data.items} />
        </CardContent>
      </Card>

      <BudgetDialog editor={editor} />
    </div>
  )
}

function AddModelBudgetButton({
  users,
  models,
  editor,
}: {
  users: SpendBudgetUserView[]
  models: ModelView[]
  editor: BudgetEditor
}) {
  const [open, setOpen] = useState(false)
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button type="button" variant="outline">
          <AppIcon icon={Add01Icon} size={14} stroke={1.5} data-icon="inline-start" />
          Add model budget
        </Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Add model budget</DialogTitle>
          <DialogDescription>
            Create a budget for one user and either a managed model id or an upstream model name.
          </DialogDescription>
        </DialogHeader>
        <UserModelBudgetForm
          users={users}
          models={models}
          editor={editor}
          onCreated={() => setOpen(false)}
        />
      </DialogContent>
    </Dialog>
  )
}

function UserCardGrid({ list, editor }: { list: UserBudgetList; editor: BudgetEditor }) {
  if (list.pageRows.length === 0) {
    return (
      <Empty className="border">
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <AppIcon icon={UserGroupIcon} size={16} stroke={1.5} />
          </EmptyMedia>
          <EmptyTitle>No users match</EmptyTitle>
          <EmptyDescription>
            {list.totalCount === 0
              ? 'No users are available.'
              : `${list.hiddenCount} of ${list.totalCount} users hidden by the current filters.`}
          </EmptyDescription>
        </EmptyHeader>
        {list.activeFilterCount > 0 ? (
          <EmptyContent>
            <Button type="button" variant="outline" size="sm" onClick={list.clearFilters}>
              Clear filters
            </Button>
          </EmptyContent>
        ) : null}
      </Empty>
    )
  }
  return (
    <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
      {list.pageRows.map((row) => (
        <UserBudgetCard key={row.user.user_id} row={row} editor={editor} />
      ))}
    </div>
  )
}

function UserBudgetCard({ row, editor }: { row: UserBudgetRow; editor: BudgetEditor }) {
  const { user, usage } = row
  return (
    <Card size="sm">
      <CardHeader className="flex flex-row items-start gap-3">
        <GeneratedAvatar kind="user" name={user.name} size={36} />
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <CardTitle className="truncate">{user.name}</CardTitle>
          <CardDescription className="truncate">{user.email}</CardDescription>
        </div>
        <UsageStatusBadge status={usage.status} />
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        <UsageBar usage={usage} />
        <dl className="grid grid-cols-2 gap-x-4 gap-y-2 text-xs">
          <UserDetail
            label="Budget"
            value={
              user.budget
                ? `${CURRENCY_FORMATTER.format(user.budget.amount_usd_10000 / 10_000)} · ${formatCadence(
                    user.budget.cadence,
                  )}`
                : 'Not set'
            }
          />
          <UserDetail
            label="Remaining"
            value={
              usage.remainingUsd === null ? '—' : CURRENCY_FORMATTER.format(usage.remainingUsd)
            }
          />
          <UserDetail
            label="Limit"
            value={user.budget ? (user.budget.hard_limit ? 'Hard' : 'Soft') : '—'}
          />
          <UserDetail label="Alerts" value={user.alert_recipient_summary} />
        </dl>
      </CardContent>
      <CardFooter className="gap-2">
        <Button type="button" size="sm" variant="secondary" onClick={() => editor.openUser(user)}>
          Configure
        </Button>
        {user.budget ? (
          <Button
            type="button"
            size="sm"
            variant="ghost"
            disabled={editor.isPending}
            onClick={() => editor.remove(userScope(user), 'User budget removed')}
          >
            Remove
          </Button>
        ) : null}
      </CardFooter>
    </Card>
  )
}

function UserDetail({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex min-w-0 flex-col gap-0.5">
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="truncate font-medium" title={value}>
        {value}
      </dd>
    </div>
  )
}

function ServiceAccountRow({
  serviceAccount,
  editor,
}: {
  serviceAccount: SpendBudgetServiceAccountView
  editor: BudgetEditor
}) {
  const usage = budgetUsage(serviceAccount)
  return (
    <li className="flex flex-wrap items-center gap-3 py-3 first:pt-0 last:pb-0">
      <IconTile variant="outline" size="sm">
        <AppIcon icon={RoboticIcon} size={14} stroke={1.5} />
      </IconTile>
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="truncate text-sm font-medium">{serviceAccount.service_account_name}</span>
        <span className="text-muted-foreground truncate text-xs">
          {serviceAccount.service_account_key} / {serviceAccount.team_name}
        </span>
        {serviceAccount.alert_email_ready ? null : (
          <span className="text-destructive text-xs">No alert email configured</span>
        )}
      </div>
      <UsageBar usage={usage} showAmounts className="w-40" />
      <UsageStatusBadge status={usage.status} />
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            type="button"
            size="icon-xs"
            variant="ghost"
            aria-label={`Budget actions for ${serviceAccount.service_account_name}`}
          >
            <AppIcon icon={Settings02Icon} size={14} stroke={1.5} />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuItem onSelect={() => editor.openServiceAccount(serviceAccount)}>
            Configure
          </DropdownMenuItem>
          <DropdownMenuItem
            variant="destructive"
            disabled={!serviceAccount.budget || editor.isPending}
            onSelect={() =>
              editor.remove(serviceAccountScope(serviceAccount), 'Service account budget removed')
            }
          >
            Remove
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </li>
  )
}

function UserModelRow({
  budget,
  ownerName,
  models,
  editor,
}: {
  budget: SpendBudgetUserModelView
  ownerName: string
  models: ModelView[]
  editor: BudgetEditor
}) {
  const usage = budgetUsage(budget)
  const selector = formatUserModelSelector(budget, models)
  return (
    <li className="flex flex-wrap items-center gap-3 py-3 first:pt-0 last:pb-0">
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="truncate text-sm font-medium">{ownerName}</span>
        <span className="text-muted-foreground truncate font-mono text-xs">{selector}</span>
      </div>
      <UsageBar usage={usage} showAmounts className="w-40" />
      <div className="flex items-center gap-1">
        <Button
          type="button"
          size="sm"
          variant="secondary"
          onClick={() => editor.openUserModel(budget, `${ownerName} / ${selector}`)}
        >
          Configure
        </Button>
        <Button
          type="button"
          size="sm"
          variant="ghost"
          disabled={editor.isPending}
          onClick={() => editor.remove(userModelScope(budget), 'User model budget removed')}
        >
          Remove
        </Button>
      </div>
    </li>
  )
}
