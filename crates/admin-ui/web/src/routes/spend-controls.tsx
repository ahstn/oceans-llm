import { useMemo } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import {
  Alert02Icon,
  AlertDiamondIcon,
  Layers01Icon,
  RoboticIcon,
  Settings02Icon,
  UserGroupIcon,
  UserIcon,
  Wallet01Icon,
} from '@hugeicons/core-free-icons'

import { AppIcon } from '@/components/icons/app-icon'
import { PageHeader } from '@/components/layout/page-header'
import { IconTile } from '@/components/reui/icon-tile'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from '@/components/ui/empty'
import { GeneratedAvatar } from '@/components/ui/generated-avatar'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { CURRENCY_FORMATTER, formatUsd10000 } from '@/lib/format'
import { cn } from '@/lib/utils'
import type {
  ModelView,
  SpendBudgetServiceAccountView,
  SpendBudgetUserModelView,
  SpendBudgetUserView,
} from '@/types/api'

import { AlertTimeline } from './spend-controls/-alert-timeline'
import { BudgetDialog, useBudgetEditor, type BudgetEditor } from './spend-controls/-budget-editor'
import {
  formatCadence,
  formatUserModelSelector,
  loadSpendControls,
  serviceAccountScope,
  serviceAccountTarget,
  userModelScope,
  userModelTarget,
  userScope,
  userTarget,
  type BudgetSource,
  type SpendControlsLoaderData,
} from './spend-controls/-budget-model'
import {
  budgetUsage,
  BudgetSourceBadge,
  summarizeUsage,
  UsageBar,
  UsageStatusBadge,
  type BudgetSummary,
} from './spend-controls/-usage'
import {
  UserListPager,
  UserListToolbar,
  useUserBudgetList,
  type UserBudgetList,
} from './spend-controls/-user-list'
import { UserModelBudgetForm } from './spend-controls/-user-model-form'

export const Route = createFileRoute('/spend-controls')({
  loader: loadSpendControls,
  component: SpendControlsPage,
})

// One tabbed card of dense tables sharing a single editor; the page wires the
// loader data into the summary strip, the three tabs, and the alert history.
// oxlint-disable-next-line eslint/max-lines-per-function
export function SpendControlsPage() {
  const { budgets, alerts, models } = Route.useLoaderData() as SpendControlsLoaderData
  const {
    users,
    service_accounts: serviceAccounts,
    user_model_budgets: userModelBudgets,
  } = budgets.data
  const editor = useBudgetEditor()
  const list = useUserBudgetList(users)
  // Users and service accounts are both budget owners; the strip covers both.
  const owners = useMemo(() => [...users, ...serviceAccounts], [users, serviceAccounts])
  const summary = useMemo(() => summarizeUsage(owners.map(budgetUsage)), [owners])
  const usersById = useMemo(() => new Map(users.map((user) => [user.user_id, user])), [users])

  return (
    <div className="flex min-w-0 flex-1 flex-col gap-6">
      <PageHeader
        section="Budget & Spending"
        title="Spend controls"
        description="Set spending limits for users, automated accounts, and each user's model use. Review recent alerts."
      />

      <SummaryStrip summary={summary} totalOwners={owners.length} />

      <Card className="min-w-0">
        <CardHeader>
          <CardTitle>Budgets</CardTitle>
          <CardDescription>
            Current window spend against each configured limit. Configure or remove budgets inline.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex min-w-0 flex-col gap-4">
          <Tabs defaultValue="users" className="gap-4">
            <TabsList>
              <TabsTrigger value="users">
                <AppIcon icon={UserIcon} size={14} stroke={1.5} data-icon="inline-start" />
                Users ({users.length})
              </TabsTrigger>
              <TabsTrigger value="service-accounts">
                <AppIcon icon={RoboticIcon} size={14} stroke={1.5} data-icon="inline-start" />
                Service accounts ({serviceAccounts.length})
              </TabsTrigger>
              <TabsTrigger value="model-budgets">
                <AppIcon icon={Layers01Icon} size={14} stroke={1.5} data-icon="inline-start" />
                Model budgets ({userModelBudgets.length})
              </TabsTrigger>
            </TabsList>

            <TabsContent value="users" className="flex min-w-0 flex-col gap-4">
              <UserListToolbar list={list} />
              <UsersTable list={list} editor={editor} />
              <UserListPager list={list} />
            </TabsContent>

            <TabsContent value="service-accounts" className="min-w-0">
              <ServiceAccountsTable serviceAccounts={serviceAccounts} editor={editor} />
            </TabsContent>

            <TabsContent value="model-budgets" className="flex min-w-0 flex-col gap-4">
              <UserModelBudgetsTable
                budgets={userModelBudgets}
                usersById={usersById}
                models={models}
                editor={editor}
              />
              <section className="flex flex-col gap-3 rounded-md border p-4">
                <div className="flex flex-col gap-1">
                  <h3 className="text-sm font-medium">Add model budget</h3>
                  <p className="text-muted-foreground text-sm">
                    Model-specific budgets are evaluated before the user's general budget.
                  </p>
                </div>
                <UserModelBudgetForm users={users} models={models} editor={editor} />
              </section>
            </TabsContent>
          </Tabs>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Alert history</CardTitle>
          <CardDescription>
            The latest threshold alerts and delivery outcomes for audit review.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <AlertTimeline items={alerts.data.items} />
        </CardContent>
      </Card>

      <BudgetDialog editor={editor} />
    </div>
  )
}

// ---------------------------------------------------------------------------
// Summary strip
// ---------------------------------------------------------------------------

function SummaryStrip({ summary, totalOwners }: { summary: BudgetSummary; totalOwners: number }) {
  return (
    <div className="grid gap-3 md:grid-cols-4">
      <StatTile
        icon={Wallet01Icon}
        label="Spend against budgets"
        value={CURRENCY_FORMATTER.format(summary.budgetedSpendUsd)}
      />
      <StatTile
        icon={UserGroupIcon}
        label="Budgeted owners"
        value={`${summary.budgeted} / ${totalOwners}`}
      />
      <StatTile
        icon={AlertDiamondIcon}
        label="Over budget"
        value={String(summary.overBudget)}
        tone={summary.overBudget > 0 ? 'destructive' : undefined}
      />
      <StatTile
        icon={Alert02Icon}
        label="Near limit"
        value={String(summary.nearLimit)}
        tone={summary.nearLimit > 0 ? 'warning' : undefined}
      />
    </div>
  )
}

const STAT_TONE_CLASS = {
  destructive: 'text-destructive',
  warning: 'text-[var(--color-warning)]',
} as const

function StatTile({
  icon,
  label,
  value,
  tone,
}: {
  icon: typeof Wallet01Icon
  label: string
  value: string
  /** Retints the soft tile; defaults to the primary tone. */
  tone?: keyof typeof STAT_TONE_CLASS
}) {
  return (
    <Card size="sm">
      <CardContent className="flex items-center gap-3">
        <IconTile variant="soft" size="sm" className={tone && STAT_TONE_CLASS[tone]}>
          <AppIcon icon={icon} size={16} stroke={1.5} />
        </IconTile>
        <div className="flex min-w-0 flex-col">
          <span className="text-muted-foreground text-xs">{label}</span>
          <span className="truncate text-lg font-semibold tabular-nums">{value}</span>
        </div>
      </CardContent>
    </Card>
  )
}

// ---------------------------------------------------------------------------
// Users tab
// ---------------------------------------------------------------------------

function UsersTable({ list, editor }: { list: UserBudgetList; editor: BudgetEditor }) {
  if (list.pageRows.length === 0) {
    return <UsersEmpty list={list} />
  }
  return (
    <div className="overflow-hidden rounded-md border">
      <Table>
        <TableHeader className="bg-muted/50">
          <TableRow>
            <TableHead className="text-muted-foreground">User</TableHead>
            <TableHead className="text-muted-foreground">Budget</TableHead>
            <TableHead className="text-muted-foreground">Usage</TableHead>
            <TableHead className="text-muted-foreground">Status</TableHead>
            <TableHead className="text-muted-foreground">Alerts</TableHead>
            <TableHead className="text-muted-foreground">Actions</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {list.pageRows.map(({ user, usage }) => (
            <TableRow key={user.user_id}>
              <TableCell>
                <div className="flex items-center gap-3">
                  <GeneratedAvatar kind="user" name={user.name} size={28} />
                  <div className="flex min-w-0 flex-col">
                    <span className="truncate font-medium">{user.name}</span>
                    <span className="text-muted-foreground truncate text-xs">
                      {user.email}
                      {user.team_name ? ` · ${user.team_name}` : ''}
                    </span>
                  </div>
                </div>
              </TableCell>
              <BudgetCell budget={user.budget} source={user.budget_source} />
              <TableCell className="min-w-[12rem]">
                <UsageBar usage={usage} />
              </TableCell>
              <TableCell>
                <UsageStatusBadge status={usage.status} />
              </TableCell>
              <TableCell>
                <p className="text-muted-foreground max-w-[14rem] truncate">
                  {user.alert_recipient_summary}
                </p>
              </TableCell>
              <ActionsCell
                disabled={editor.isPending}
                onConfigure={() => editor.open(userTarget(user))}
                onRemove={
                  user.budget
                    ? () => editor.remove(userScope(user), 'User budget removed')
                    : undefined
                }
              />
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}

function UsersEmpty({ list }: { list: UserBudgetList }) {
  const filtered = list.totalCount > 0
  return (
    <Empty className="border">
      <EmptyHeader>
        <EmptyMedia variant="icon">
          <AppIcon icon={UserGroupIcon} size={22} stroke={1.5} />
        </EmptyMedia>
        <EmptyTitle>
          {filtered ? 'No users match these filters' : 'No users are available'}
        </EmptyTitle>
        <EmptyDescription>
          {filtered
            ? `${list.hiddenCount} users are hidden by the current search and filters.`
            : 'Users appear here once they have been provisioned.'}
        </EmptyDescription>
      </EmptyHeader>
      {filtered ? (
        <EmptyContent>
          <Button type="button" variant="outline" size="sm" onClick={list.clearFilters}>
            Clear filters
          </Button>
        </EmptyContent>
      ) : null}
    </Empty>
  )
}

// ---------------------------------------------------------------------------
// Service accounts tab
// ---------------------------------------------------------------------------

function ServiceAccountsTable({
  serviceAccounts,
  editor,
}: {
  serviceAccounts: SpendBudgetServiceAccountView[]
  editor: BudgetEditor
}) {
  if (serviceAccounts.length === 0) {
    return (
      <Empty className="border">
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <AppIcon icon={RoboticIcon} size={22} stroke={1.5} />
          </EmptyMedia>
          <EmptyTitle>No service accounts are available</EmptyTitle>
          <EmptyDescription>
            Active service-account keys require an active service-account budget.
          </EmptyDescription>
        </EmptyHeader>
      </Empty>
    )
  }
  return (
    <div className="overflow-hidden rounded-md border">
      <Table>
        <TableHeader className="bg-muted/50">
          <TableRow>
            <TableHead className="text-muted-foreground">Service account</TableHead>
            <TableHead className="text-muted-foreground">Budget</TableHead>
            <TableHead className="text-muted-foreground">Usage</TableHead>
            <TableHead className="text-muted-foreground">Status</TableHead>
            <TableHead className="text-muted-foreground">Alerts</TableHead>
            <TableHead className="text-muted-foreground">Actions</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {serviceAccounts.map((serviceAccount) => {
            const usage = budgetUsage(serviceAccount)
            return (
              <TableRow key={serviceAccount.service_account_id}>
                <TableCell>
                  <div className="flex min-w-0 flex-col">
                    <span className="truncate font-medium">
                      {serviceAccount.service_account_name}
                    </span>
                    <span className="text-muted-foreground truncate font-mono text-xs">
                      {serviceAccount.service_account_key} / {serviceAccount.team_name}
                    </span>
                  </div>
                </TableCell>
                <BudgetCell budget={serviceAccount.budget} source={serviceAccount.budget_source} />
                <TableCell className="min-w-[12rem]">
                  <UsageBar usage={usage} />
                </TableCell>
                <TableCell>
                  <UsageStatusBadge status={usage.status} />
                </TableCell>
                <TableCell>
                  <p
                    className={cn(
                      'max-w-[14rem] truncate',
                      serviceAccount.alert_email_ready
                        ? 'text-muted-foreground'
                        : 'text-destructive',
                    )}
                  >
                    {serviceAccount.alert_recipient_summary}
                  </p>
                </TableCell>
                <ActionsCell
                  disabled={editor.isPending}
                  onConfigure={() => editor.open(serviceAccountTarget(serviceAccount))}
                  onRemove={
                    serviceAccount.budget
                      ? () =>
                          editor.remove(
                            serviceAccountScope(serviceAccount),
                            'Service account budget removed',
                          )
                      : undefined
                  }
                />
              </TableRow>
            )
          })}
        </TableBody>
      </Table>
    </div>
  )
}

// ---------------------------------------------------------------------------
// Model budgets tab
// ---------------------------------------------------------------------------

function UserModelBudgetsTable({
  budgets,
  usersById,
  models,
  editor,
}: {
  budgets: SpendBudgetUserModelView[]
  usersById: Map<string, SpendBudgetUserView>
  models: ModelView[]
  editor: BudgetEditor
}) {
  if (budgets.length === 0) {
    return (
      <Empty className="border">
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <AppIcon icon={Layers01Icon} size={22} stroke={1.5} />
          </EmptyMedia>
          <EmptyTitle>No user model budgets are configured</EmptyTitle>
          <EmptyDescription>
            Add one below to cap a single user's spend on one managed model or upstream model.
          </EmptyDescription>
        </EmptyHeader>
      </Empty>
    )
  }
  return (
    <div className="overflow-hidden rounded-md border">
      <Table>
        <TableHeader className="bg-muted/50">
          <TableRow>
            <TableHead className="text-muted-foreground">User</TableHead>
            <TableHead className="text-muted-foreground">Scope</TableHead>
            <TableHead className="text-muted-foreground">Budget</TableHead>
            <TableHead className="text-muted-foreground">Usage</TableHead>
            <TableHead className="text-muted-foreground">Actions</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {budgets.map((budget) => {
            const owner = usersById.get(budget.user_id)
            const ownerName = owner?.name ?? budget.user_id
            const selector = formatUserModelSelector(budget, models)
            return (
              <TableRow key={budget.scope_key}>
                <TableCell>
                  <div className="flex min-w-0 flex-col">
                    <span className="truncate font-medium">{ownerName}</span>
                    <span className="text-muted-foreground truncate text-xs">
                      {owner?.email ?? budget.user_id}
                    </span>
                  </div>
                </TableCell>
                <TableCell className="font-mono text-xs">{selector}</TableCell>
                <BudgetCell budget={budget.budget} source={budget.budget_source} />
                <TableCell className="min-w-[12rem]">
                  <UsageBar usage={budgetUsage(budget)} />
                </TableCell>
                <ActionsCell
                  disabled={editor.isPending}
                  onConfigure={() =>
                    editor.open(userModelTarget(budget, `${ownerName} / ${selector}`))
                  }
                  onRemove={() =>
                    editor.remove(userModelScope(budget), 'User model budget removed')
                  }
                />
              </TableRow>
            )
          })}
        </TableBody>
      </Table>
    </div>
  )
}

// ---------------------------------------------------------------------------
// Shared cells
// ---------------------------------------------------------------------------

function BudgetCell({
  budget,
  source,
}: {
  budget: SpendBudgetUserView['budget']
  source: BudgetSource | null | undefined
}) {
  if (!budget) {
    return (
      <TableCell>
        <span className="text-muted-foreground">Not set</span>
      </TableCell>
    )
  }
  return (
    <TableCell>
      <div className="flex flex-wrap items-center gap-1.5">
        <span className="font-medium tabular-nums">{formatUsd10000(budget.amount_usd_10000)}</span>
        <Badge variant="secondary">{formatCadence(budget.cadence)}</Badge>
        {budget.hard_limit ? <Badge variant="outline">Hard</Badge> : null}
        <BudgetSourceBadge source={source} />
      </div>
    </TableCell>
  )
}

function ActionsCell({
  disabled,
  onConfigure,
  onRemove,
}: {
  disabled: boolean
  onConfigure: () => void
  /** Omitted when the row has no budget to remove. */
  onRemove?: () => void
}) {
  return (
    <TableCell>
      <div className="flex items-center gap-1">
        <Button type="button" size="sm" variant="outline" onClick={onConfigure}>
          <AppIcon icon={Settings02Icon} size={14} stroke={1.5} data-icon="inline-start" />
          Configure
        </Button>
        {onRemove ? (
          <Button type="button" size="sm" variant="ghost" disabled={disabled} onClick={onRemove}>
            Remove
          </Button>
        ) : null}
      </div>
    </TableCell>
  )
}
