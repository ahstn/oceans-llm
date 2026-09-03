import { useMemo, useState } from 'react'
import { FilterHorizontalIcon, Search01Icon } from '@hugeicons/core-free-icons'

import { AppIcon } from '@/components/icons/app-icon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldGroup,
  FieldLabel,
} from '@/components/ui/field'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'
import {
  Pagination,
  PaginationContent,
  PaginationItem,
  PaginationNext,
  PaginationPrevious,
} from '@/components/ui/pagination'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import type { SpendBudgetUserView } from '@/types/api'

import {
  budgetUsage,
  isQuietUsage,
  LOW_USAGE_RATIO,
  PERCENT_FORMATTER,
  type BudgetUsage,
} from './-usage'

// The users tab is a client-side list: every user is loaded, then filtered, sorted,
// and paged here. Quiet users are hidden by default so active spend stays in view.

export const USER_PAGE_SIZE = 15

export type BudgetStateFilter = 'all' | 'budgeted' | 'unbudgeted'
export type UserSortKey = 'spend' | 'usage' | 'name'

export type UserListFilters = {
  query: string
  /** Hide users that `isQuietUsage` says need no attention right now. */
  hideQuiet: boolean
  budgetState: BudgetStateFilter
  sort: UserSortKey
}

const DEFAULT_FILTERS: UserListFilters = {
  query: '',
  hideQuiet: true,
  budgetState: 'all',
  sort: 'spend',
}

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
    if (filters.hideQuiet && isQuietUsage(usage)) return false
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

export function useUserBudgetList(users: SpendBudgetUserView[]): UserBudgetList {
  const [filters, setFiltersState] = useState<UserListFilters>(DEFAULT_FILTERS)
  const [page, setPage] = useState(1)

  const allRows = useMemo(() => toUserRows(users), [users])
  const visibleRows = useMemo(
    () => sortUserRows(filterUserRows(allRows, filters), filters.sort),
    [allRows, filters],
  )
  const totalPages = Math.max(1, Math.ceil(visibleRows.length / USER_PAGE_SIZE))
  const currentPage = Math.min(page, totalPages)
  const pageRows = visibleRows.slice(
    (currentPage - 1) * USER_PAGE_SIZE,
    currentPage * USER_PAGE_SIZE,
  )

  function setFilters(update: Partial<UserListFilters>) {
    setFiltersState((current) => ({ ...current, ...update }))
    setPage(1)
  }

  return {
    filters,
    setFilters,
    clearFilters: () => setFilters({ query: '', hideQuiet: false, budgetState: 'all' }),
    page: currentPage,
    setPage,
    totalPages,
    pageRows,
    visibleCount: visibleRows.length,
    totalCount: allRows.length,
    hiddenCount: allRows.length - visibleRows.length,
    activeFilterCount:
      Number(filters.hideQuiet) +
      Number(filters.budgetState !== 'all') +
      Number(filters.query.trim().length > 0),
  }
}

// ---------------------------------------------------------------------------
// Toolbar: search, filters popover, sort.
// ---------------------------------------------------------------------------

// oxlint-disable-next-line eslint/max-lines-per-function
export function UserListToolbar({ list }: { list: UserBudgetList }) {
  return (
    <div className="flex flex-wrap items-center gap-2">
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
              Quiet users are hidden by default so the list stays focused on active spend.
            </p>
          </div>
          <FieldGroup className="gap-3">
            <Field orientation="horizontal">
              <Checkbox
                id="filter-hide-quiet"
                checked={list.filters.hideQuiet}
                onCheckedChange={(checked) => list.setFilters({ hideQuiet: checked === true })}
              />
              <FieldContent>
                <FieldLabel htmlFor="filter-hide-quiet">Hide quiet users</FieldLabel>
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
                  if (value) list.setFilters({ budgetState: value as BudgetStateFilter })
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

export function UserListPager({ list }: { list: UserBudgetList }) {
  const from = list.visibleCount === 0 ? 0 : (list.page - 1) * USER_PAGE_SIZE + 1
  const to = Math.min(list.page * USER_PAGE_SIZE, list.visibleCount)
  return (
    <div className="flex flex-wrap items-center justify-between gap-3">
      <p className="text-muted-foreground text-sm">
        Showing {from}–{to} of {list.visibleCount} users
        {list.hiddenCount > 0 ? ` · ${list.hiddenCount} hidden by filters` : ''}
      </p>
      <Pagination className="mx-0 w-auto">
        <PaginationContent>
          {list.page > 1 ? (
            <PaginationItem>
              <PaginationPrevious
                href="#"
                onClick={(event) => {
                  event.preventDefault()
                  list.setPage(list.page - 1)
                }}
              />
            </PaginationItem>
          ) : null}
          <PaginationItem>
            <span className="text-muted-foreground px-2 text-sm">
              Page {list.page} of {list.totalPages}
            </span>
          </PaginationItem>
          {list.page < list.totalPages ? (
            <PaginationItem>
              <PaginationNext
                href="#"
                onClick={(event) => {
                  event.preventDefault()
                  list.setPage(list.page + 1)
                }}
              />
            </PaginationItem>
          ) : null}
        </PaginationContent>
      </Pagination>
    </div>
  )
}
