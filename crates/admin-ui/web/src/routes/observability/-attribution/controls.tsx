import { Cancel01Icon, RefreshIcon } from '@hugeicons/core-free-icons'

import { AppIcon } from '@/components/icons/app-icon'
import { Button } from '@/components/ui/button'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'

import { ATTRIBUTION_WINDOWS, toAttributionWindow, type AttributionWindowDays } from './shared'

/** Normalised repositories always contain a `/`, so this never names one. */
const ALL_REPOSITORIES = 'all'

/** Window toggle, repository filter, and refresh, above the report like Usage Costs' controls. */
export function AttributionFilters({
  windowDays,
  repository,
  repositories,
  isPending,
  onWindowChange,
  onRepositoryChange,
  onRefresh,
}: {
  windowDays: AttributionWindowDays
  repository: string | undefined
  /** Repositories to offer, including the active filter. */
  repositories: string[]
  isPending: boolean
  onWindowChange: (days: AttributionWindowDays) => void
  onRepositoryChange: (repository: string | undefined) => void
  onRefresh: () => void
}) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      <ToggleGroup
        type="single"
        variant="outline"
        size="sm"
        value={String(windowDays)}
        onValueChange={(value) => {
          const days = toAttributionWindow(value)
          if (days) onWindowChange(days)
        }}
        disabled={isPending}
        aria-label="Report window"
      >
        {ATTRIBUTION_WINDOWS.map((days) => (
          <ToggleGroupItem key={days} value={String(days)} aria-label={`Last ${days} days`}>
            {days}d
          </ToggleGroupItem>
        ))}
      </ToggleGroup>
      <Select
        value={repository ?? ALL_REPOSITORIES}
        onValueChange={(value) =>
          onRepositoryChange(value === ALL_REPOSITORIES ? undefined : value)
        }
        disabled={isPending}
      >
        <SelectTrigger size="sm" className="w-56 text-xs" aria-label="Repository filter">
          <SelectValue placeholder="Repository" />
        </SelectTrigger>
        <SelectContent>
          <SelectGroup>
            <SelectItem value={ALL_REPOSITORIES}>All repositories</SelectItem>
            {repositories.map((name) => (
              <SelectItem key={name} value={name}>
                {name}
              </SelectItem>
            ))}
          </SelectGroup>
        </SelectContent>
      </Select>
      <Button
        type="button"
        variant="outline"
        size="sm"
        className="gap-2"
        onClick={onRefresh}
        disabled={isPending}
      >
        <AppIcon icon={RefreshIcon} size={14} stroke={1.5} data-icon="inline-start" />
        {isPending ? 'Refreshing...' : 'Refresh'}
      </Button>
    </div>
  )
}

/** The active user filter, set from a User activity row; pressing it clears the filter. */
export function UserFilterButton({
  name,
  isPending,
  onClear,
}: {
  name: string
  isPending: boolean
  onClear: () => void
}) {
  return (
    <Button
      type="button"
      variant="secondary"
      size="sm"
      onClick={onClear}
      disabled={isPending}
      aria-label={`Clear user filter: ${name}`}
      title="Clear the user filter"
    >
      {name}
      <AppIcon icon={Cancel01Icon} size={14} stroke={1.5} data-icon="inline-end" aria-hidden />
    </Button>
  )
}
