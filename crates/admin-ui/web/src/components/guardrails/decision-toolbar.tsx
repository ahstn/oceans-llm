import { Cancel01Icon, Search01Icon } from '@hugeicons/core-free-icons'

import { AppIcon } from '@/components/icons/app-icon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  InputGroup,
  InputGroupAddon,
  InputGroupButton,
  InputGroupInput,
} from '@/components/ui/input-group'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import type {
  GuardrailDecisionFiltersInput,
  GuardrailDecisionPageView,
  GuardrailPoliciesView,
} from '@/types/api'

import { ACTIONS, PHASES } from './decision-meta'

export type DashboardProps = {
  policies: GuardrailPoliciesView
  decisions: GuardrailDecisionPageView
  search: GuardrailDecisionFiltersInput
  pending: boolean
  /** Replaces the server-side filters and returns to the first page. */
  onSearch: (filters: GuardrailDecisionFiltersInput) => void
  onPage: (page: number) => void
}

type ToolbarProps = Pick<DashboardProps, 'search' | 'pending' | 'onSearch'> & {
  showPhase?: boolean
}

/** Server-side filters every candidate shares: action toggles, phase, and a request lookup. */
export function DecisionToolbar({ search, pending, onSearch, showPhase = true }: ToolbarProps) {
  const update = (patch: GuardrailDecisionFiltersInput) => {
    const next: GuardrailDecisionFiltersInput = { ...search, ...patch, page: undefined }
    for (const key of Object.keys(next) as (keyof GuardrailDecisionFiltersInput)[]) {
      if (next[key] === '' || next[key] === undefined) delete next[key]
    }
    onSearch(next)
  }
  const hasFilters = Boolean(search.action || search.phase || search.request_id || search.evaluator)

  return (
    <div className="flex flex-wrap items-center gap-2">
      <form
        key={search.request_id ?? ''}
        className="w-full sm:w-64"
        action={(formData) => {
          const value = formData.get('request_id')
          update({ request_id: typeof value === 'string' ? value.trim() : '' })
        }}
      >
        <InputGroup>
          <InputGroupInput
            name="request_id"
            aria-label="Request ID"
            placeholder="Request ID…"
            defaultValue={search.request_id ?? ''}
          />
          <InputGroupAddon>
            <AppIcon icon={Search01Icon} aria-hidden />
          </InputGroupAddon>
          {search.request_id ? (
            <InputGroupAddon align="inline-end">
              <InputGroupButton
                aria-label="Clear request ID"
                size="icon-xs"
                onClick={() => update({ request_id: '' })}
              >
                <AppIcon icon={Cancel01Icon} aria-hidden />
              </InputGroupButton>
            </InputGroupAddon>
          ) : null}
        </InputGroup>
      </form>
      <ToggleGroup
        type="single"
        variant="outline"
        size="sm"
        aria-label="Action"
        disabled={pending}
        value={search.action || 'all'}
        onValueChange={(value) => update({ action: value === 'all' || !value ? '' : value })}
      >
        <ToggleGroupItem value="all">All</ToggleGroupItem>
        {ACTIONS.map(({ id, label }) => (
          <ToggleGroupItem key={id} value={id}>
            {label}
          </ToggleGroupItem>
        ))}
      </ToggleGroup>
      {showPhase ? (
        <Select
          value={search.phase || 'all'}
          disabled={pending}
          onValueChange={(value) => update({ phase: value === 'all' ? '' : value })}
        >
          <SelectTrigger size="sm" aria-label="Phase" className="w-44">
            <SelectValue placeholder="All phases" />
          </SelectTrigger>
          <SelectContent>
            <SelectGroup>
              <SelectItem value="all">All phases</SelectItem>
              {PHASES.map(({ id, label }) => (
                <SelectItem key={id} value={id}>
                  {label}
                </SelectItem>
              ))}
            </SelectGroup>
          </SelectContent>
        </Select>
      ) : null}
      {search.evaluator ? <Badge variant="outline">Evaluator: {search.evaluator}</Badge> : null}
      {hasFilters ? (
        <Button variant="ghost" size="sm" disabled={pending} onClick={() => onSearch({})}>
          Clear
        </Button>
      ) : null}
    </div>
  )
}

/** Previous/next paging for server pages; hidden when everything fits on one page. */
export function DecisionPager({
  decisions,
  pending,
  onPage,
}: Pick<DashboardProps, 'decisions' | 'pending' | 'onPage'>) {
  const { page, page_size: pageSize, total, items } = decisions
  const first = total === 0 ? 0 : (page - 1) * pageSize + 1
  return (
    <div className="text-muted-foreground flex items-center justify-between gap-2 text-xs">
      <span className="tabular-nums">
        {items.length ? `${first}–${first + items.length - 1}` : '0'} of {total.toLocaleString()}
      </span>
      {total > pageSize ? (
        <div className="flex gap-2">
          <Button
            variant="outline"
            size="sm"
            disabled={pending || page <= 1}
            onClick={() => onPage(page - 1)}
          >
            Previous
          </Button>
          <Button
            variant="outline"
            size="sm"
            disabled={pending || page * pageSize >= total}
            onClick={() => onPage(page + 1)}
          >
            Next
          </Button>
        </div>
      ) : null}
    </div>
  )
}
