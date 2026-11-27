import type { ReactNode } from 'react'

import type { FilterPatch } from '@/components/data/filter-fields'
import {
  useLoadedFilterOptions,
  useModelFilterOptions,
  type FilterOption,
  type FilterOptionsState,
} from '@/components/data/filter-options'
import { FilterToolbar } from '@/components/data/filter-toolbar'
import { getObservabilityHarnessUsage } from '@/server/admin-data.functions'

import {
  sessionFilterGroups,
  type SessionFilterParam,
  type SessionFilterValues,
} from './session-filter-fields'

/** Chip drafts and URL patches. The route turns `minimum_coverage_percent` back into a number. */
export type SessionFilterPatch = FilterPatch<SessionFilterParam>

interface SessionFilterToolbarProps {
  filters: SessionFilterValues
  shownCount: number
  totalCount: number
  isPending: boolean
  /** Controls that sit beside `Filters`, such as the column toggle. */
  actions?: ReactNode
  /** Merges `patch` into the current URL search; keys set to `undefined` are removed. */
  onApply: (patch: SessionFilterPatch) => void
}

/** The agent session toolbar: the shared filter toolbar over the session fields. */
export function SessionFilterToolbar({ filters, ...props }: SessionFilterToolbarProps) {
  return (
    <FilterToolbar
      groups={sessionFilterGroups}
      filters={filters as SessionFilterPatch}
      testIdPrefix="agent-session"
      useCatalogOptions={useSessionCatalogOptions}
      {...props}
    />
  )
}

/** Loads the harness and model catalogs only once their chip is shown. */
function useSessionCatalogOptions(shows: (source: string) => boolean) {
  const harnessOptions = useLoadedFilterOptions(shows('harnesses'), loadHarnessOptions)
  const modelOptions = useModelFilterOptions(shows('models'))

  return (source: string): FilterOptionsState | undefined => {
    if (source === 'harnesses') return harnessOptions
    if (source === 'models') return modelOptions
    return undefined
  }
}

/** Harnesses that sent traffic in the last 31 days; older keys can still be typed. */
async function loadHarnessOptions(signal: AbortSignal): Promise<FilterOption[]> {
  const usage = await getObservabilityHarnessUsage({ data: { range: '31d' }, signal })
  return usage.data.leaders
    .map((leader) => ({
      value: leader.agent_harness_key,
      label: leader.agent_harness_label,
      hint: leader.agent_harness_key,
    }))
    .sort((left, right) => left.label.localeCompare(right.label))
}
