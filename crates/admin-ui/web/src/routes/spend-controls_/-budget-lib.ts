import { useMemo, useState } from 'react'

import { getBudgetAlertHistory, getModels, getSpendBudgets } from '@/server/admin-data.functions'
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

export const USER_PAGE_SIZE = 15
export const LOW_USAGE_RATIO = 0.2
export const WARNING_USAGE_RATIO = 0.8

export const CURRENCY_FORMATTER = new Intl.NumberFormat('en-US', {
  style: 'currency',
  currency: 'USD',
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
})

export const PERCENT_FORMATTER = new Intl.NumberFormat('en-US', {
  style: 'percent',
  maximumFractionDigits: 0,
})

export type SpendControlsLoaderData = {
  budgets: { data: SpendBudgetsView }
  alerts: { data: BudgetAlertHistoryView }
  models: { data: { items: ModelView[] } }
}

export async function loadSpendControls(): Promise<SpendControlsLoaderData> {
  const [budgets, alerts, models] = await Promise.all([
    getSpendBudgets(),
    getBudgetAlertHistory({
      data: {
        page: 1,
        page_size: 10,
        owner_kind: 'all',
        status: 'all',
        channel: 'all',
      },
    }),
    getModels({ data: { page: 1, page_size: 200 } }),
  ])
  return { budgets, alerts, models } as SpendControlsLoaderData
}

export type BudgetSettingsForm = Omit<UpsertBudgetInput, 'scope'>

export const initialBudgetSettings: BudgetSettingsForm = {
  cadence: 'daily',
  amount_usd: '100.0000',
  hard_limit: true,
  timezone: 'UTC',
}

export type BudgetSettings = NonNullable<SpendBudgetUserView['budget']>

export type BudgetOwner = {
  budget?: BudgetSettings | null
  current_window_spend_usd_10000: number
}

/** Usage state derived from one owner's budget and current window spend. */
export type UsageStatus = 'no_budget' | 'low' | 'on_track' | 'warning' | 'over'

export type BudgetUsage = {
  /** Spend divided by budget. `null` when no budget is set. */
  ratio: number | null
  status: UsageStatus
  spendUsd: number
  budgetUsd: number | null
  remainingUsd: number | null
}

export function budgetUsage(owner: BudgetOwner): BudgetUsage {
  const spendUsd = owner.current_window_spend_usd_10000 / 10_000
  if (!owner.budget || owner.budget.amount_usd_10000 <= 0) {
    return {
      ratio: null,
      status: 'no_budget',
      spendUsd,
      budgetUsd: null,
      remainingUsd: null,
    }
  }
  const budgetUsd = owner.budget.amount_usd_10000 / 10_000
  const ratio = spendUsd / budgetUsd
  return {
    ratio,
    status: usageStatus(ratio),
    spendUsd,
    budgetUsd,
    remainingUsd: Math.max(0, budgetUsd - spendUsd),
  }
}

function usageStatus(ratio: number): UsageStatus {
  if (ratio >= 1) return 'over'
  if (ratio >= WARNING_USAGE_RATIO) return 'warning'
  if (ratio >= LOW_USAGE_RATIO) return 'on_track'
  return 'low'
}

export const USAGE_STATUS_LABEL: Record<UsageStatus, string> = {
  no_budget: 'No budget',
  low: 'Low usage',
  on_track: 'On track',
  warning: 'Near limit',
  over: 'Over budget',
}

export type BudgetStateFilter = 'all' | 'budgeted' | 'unbudgeted'
export type UserSortKey = 'spend' | 'usage' | 'name'

export type UserListFilters = {
  query: string
  /**
   * Hide users whose spend is below `LOW_USAGE_RATIO` of their budget, plus users with no
   * budget and no spend. Both groups need no attention right now.
   */
  hideLowUsage: boolean
  budgetState: BudgetStateFilter
  sort: UserSortKey
}

export const DEFAULT_USER_FILTERS: UserListFilters = {
  query: '',
  hideLowUsage: true,
  budgetState: 'all',
  sort: 'spend',
}

export type UserBudgetRow = {
  user: SpendBudgetUserView
  usage: BudgetUsage
}

export function toUserRows(users: SpendBudgetUserView[]): UserBudgetRow[] {
  return users.map((user) => ({ user, usage: budgetUsage(user) }))
}

export function filterUserRows(rows: UserBudgetRow[], filters: UserListFilters) {
  const query = filters.query.trim().toLowerCase()
  return rows.filter(({ user, usage }) => {
    const isIdle = usage.status === 'low' || (usage.ratio === null && usage.spendUsd === 0)
    if (filters.hideLowUsage && isIdle) return false
    if (filters.budgetState === 'budgeted' && usage.ratio === null) return false
    if (filters.budgetState === 'unbudgeted' && usage.ratio !== null) return false
    if (query.length > 0) {
      const haystack = `${user.name} ${user.email} ${user.team_name ?? ''}`.toLowerCase()
      if (!haystack.includes(query)) return false
    }
    return true
  })
}

export function sortUserRows(rows: UserBudgetRow[], sort: UserSortKey) {
  const sorted = [...rows]
  switch (sort) {
    case 'name':
      sorted.sort((a, b) => a.user.name.localeCompare(b.user.name))
      break
    case 'usage':
      // Unbudgeted users sink to the bottom, ordered by raw spend.
      sorted.sort(
        (a, b) =>
          (b.usage.ratio ?? -1) - (a.usage.ratio ?? -1) || b.usage.spendUsd - a.usage.spendUsd,
      )
      break
    case 'spend':
      sorted.sort((a, b) => b.usage.spendUsd - a.usage.spendUsd)
      break
  }
  return sorted
}

/** Filtered, sorted, and paged view over the loaded users, plus its controls. */
export interface UserBudgetList {
  filters: UserListFilters
  setFilters: (update: Partial<UserListFilters>) => void
  /** Drop every filter so all users show. Keeps the sort order. */
  clearFilters: () => void
  page: number
  setPage: (page: number) => void
  totalPages: number
  pageRows: UserBudgetRow[]
  visibleCount: number
  totalCount: number
  hiddenCount: number
  activeFilterCount: number
}

export function useUserBudgetList(
  users: SpendBudgetUserView[],
  pageSize = USER_PAGE_SIZE,
): UserBudgetList {
  const [filters, setFiltersState] = useState<UserListFilters>(DEFAULT_USER_FILTERS)
  const [page, setPage] = useState(1)

  const allRows = useMemo(() => toUserRows(users), [users])
  const visibleRows = useMemo(
    () => sortUserRows(filterUserRows(allRows, filters), filters.sort),
    [allRows, filters],
  )
  const totalPages = Math.max(1, Math.ceil(visibleRows.length / pageSize))
  const currentPage = Math.min(page, totalPages)
  const pageRows = visibleRows.slice((currentPage - 1) * pageSize, currentPage * pageSize)

  function setFilters(update: Partial<UserListFilters>) {
    setFiltersState((current) => ({ ...current, ...update }))
    setPage(1)
  }

  return {
    filters,
    setFilters,
    clearFilters: () => setFilters({ query: '', hideLowUsage: false, budgetState: 'all' }),
    page: currentPage,
    setPage,
    totalPages,
    pageRows,
    visibleCount: visibleRows.length,
    totalCount: allRows.length,
    hiddenCount: allRows.length - visibleRows.length,
    activeFilterCount:
      Number(filters.hideLowUsage) +
      Number(filters.budgetState !== 'all') +
      Number(filters.query.trim().length > 0),
  }
}

export type BudgetSummary = {
  totalSpendUsd: number
  budgetedUsers: number
  overBudget: number
  nearLimit: number
}

export function summarizeUsers(rows: UserBudgetRow[]): BudgetSummary {
  return rows.reduce<BudgetSummary>(
    (summary, { usage }) => ({
      totalSpendUsd: summary.totalSpendUsd + usage.spendUsd,
      budgetedUsers: summary.budgetedUsers + (usage.ratio === null ? 0 : 1),
      overBudget: summary.overBudget + (usage.status === 'over' ? 1 : 0),
      nearLimit: summary.nearLimit + (usage.status === 'warning' ? 1 : 0),
    }),
    { totalSpendUsd: 0, budgetedUsers: 0, overBudget: 0, nearLimit: 0 },
  )
}

export function settingsFromBudget(budget?: BudgetSettings | null): BudgetSettingsForm {
  return {
    cadence: budget?.cadence ?? 'daily',
    amount_usd: budget?.amount_usd ?? '0.0000',
    hard_limit: budget?.hard_limit ?? true,
    timezone: budget?.timezone ?? 'UTC',
  }
}

export function budgetPayload(
  scope: BudgetScopeRequest,
  settings: BudgetSettingsForm,
): UpsertBudgetInput {
  return {
    scope,
    cadence: settings.cadence,
    amount_usd: settings.amount_usd,
    hard_limit: settings.hard_limit,
    timezone: settings.timezone?.trim() || 'UTC',
  }
}

export function userScope(user: SpendBudgetUserView): BudgetScopeRequest {
  return { kind: 'user', user_id: user.user_id }
}

export function serviceAccountScope(
  serviceAccount: SpendBudgetServiceAccountView,
): BudgetScopeRequest {
  return {
    kind: 'service_account',
    service_account_id: serviceAccount.service_account_id,
  }
}

export function userModelScope(budget: SpendBudgetUserModelView): BudgetScopeRequest {
  if (budget.model_id) {
    return {
      kind: 'user_model',
      user_id: budget.user_id,
      model_id: budget.model_id,
    }
  }
  return {
    kind: 'user_model',
    user_id: budget.user_id,
    upstream_model: budget.upstream_model ?? '',
  }
}

/**
 * Human label for a model budget scope. `ModelView.model_id` is the UUID the budget
 * stores; `ModelView.id` is the model key shown in the UI. Falls back to the raw id.
 */
export function formatUserModelSelector(
  budget: SpendBudgetUserModelView,
  models: readonly ModelView[] = [],
) {
  if (budget.model_id) {
    const key = models.find((model) => model.model_id === budget.model_id)?.id
    return `model:${key ?? budget.model_id}`
  }
  return `upstream:${budget.upstream_model ?? ''}`
}

export function formatCadence(cadence: string) {
  return cadence.charAt(0).toUpperCase() + cadence.slice(1)
}

export function formatOwnerKind(ownerKind: string) {
  return ownerKind === 'service_account' ? 'service account' : ownerKind
}

export function formatThreshold(alert: BudgetAlertHistoryItemView) {
  return `${alert.threshold_bps / 100}%`
}

export function alertBadgeVariant(
  alert: BudgetAlertHistoryItemView,
): 'default' | 'success' | 'warning' | 'destructive' {
  switch (alert.delivery_status) {
    case 'sent':
      return 'success'
    case 'pending':
      return 'warning'
    case 'failed':
      return 'destructive'
    default:
      return 'default'
  }
}

export function getErrorMessage(error: unknown) {
  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message
  }
  return 'Request failed'
}
