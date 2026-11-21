import {
  Fragment,
  useEffect,
  useRef,
  useState,
  type ChangeEvent,
  type KeyboardEvent,
  type ReactNode,
} from 'react'
import { Add01Icon, Cancel01Icon, FilterHorizontalIcon } from '@hugeicons/core-free-icons'

import {
  readyFilterOptions,
  useLoadedFilterOptions,
  useModelFilterOptions,
  type FilterOption,
  type FilterOptionsState,
} from '@/components/data/filter-options'
import { FilterValuePicker } from '@/components/data/filter-value-picker'
import { AppIcon } from '@/components/icons/app-icon'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import {
  InputGroup,
  InputGroupAddon,
  InputGroupButton,
  InputGroupInput,
  InputGroupText,
} from '@/components/ui/input-group'
import { formatCount } from '@/lib/format'
import { getObservabilityHarnessUsage } from '@/server/admin-data.functions'

import {
  dayOptions,
  fieldParams,
  sessionFilterFields,
  sessionFilterGroups,
  sessionFilterParams,
  type PickerSource,
  type SessionFilterField,
  type SessionFilterFieldKey,
  type SessionFilterParam,
  type SessionFilterValues,
} from './session-filter-fields'

/** Chip drafts and URL patches. The route turns `minimum_coverage_percent` back into a number. */
export type SessionFilterPatch = {
  [Param in SessionFilterParam]?: string | number | undefined
}

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

/**
 * The agent session toolbar: a `Filters` menu that adds removable `[field | is | value | ×]`
 * chips. Pickers apply as soon as a value is chosen; ID and tag chips apply on Enter or blur.
 */
// Chip drafts, pickers, and the add-filter menus share one URL-backed filter state.
// oxlint-disable-next-line eslint/max-lines-per-function
export function SessionFilterToolbar({
  filters,
  shownCount,
  totalCount,
  isPending,
  actions,
  onApply,
}: SessionFilterToolbarProps) {
  const [drafts, setDrafts] = useState<SessionFilterPatch>(filters)
  const [addedFields, setAddedFields] = useState<SessionFilterFieldKey[]>([])
  const [focusField, setFocusField] = useState<SessionFilterFieldKey | null>(null)
  const [syncedFilters, setSyncedFilters] = useState(filters)

  // Adopt URL-backed filter changes (navigation, Back/Forward, Clear) without discarding chip
  // edits that were typed while a previous navigation was still loading.
  if (syncedFilters !== filters) {
    setSyncedFilters(filters)
    setDrafts((current) => rebaseDrafts(current, syncedFilters, filters))
  }

  const visibleFields = sessionFilterFields.filter(
    (field) => isFieldActive(filters, field.key) || addedFields.includes(field.key),
  )
  const activeCount = sessionFilterFields.filter((field) =>
    isFieldActive(filters, field.key),
  ).length
  const pickerOptions = usePickerOptions(visibleFields)

  function addField(key: SessionFilterFieldKey) {
    setAddedFields((current) => (current.includes(key) ? current : [...current, key]))
    setFocusField(key)
  }

  // A half-typed tag pair stays a draft; every other change, such as a pick, still applies.
  function commit(next: SessionFilterPatch = drafts) {
    const patch = pickFilterParams(next)
    if (isPartialTag(next)) {
      delete patch.request_tag_key
      delete patch.request_tag_value
    }
    if (!sameFilters({ ...filters, ...patch }, filters)) {
      onApply(patch)
    }
  }

  // Picked values have no draft stage, so they apply as soon as they are chosen.
  function pick(param: SessionFilterParam, value: string) {
    const next = { ...drafts, [param]: value }
    setDrafts(next)
    commit(next)
  }

  function removeField(key: SessionFilterFieldKey) {
    setAddedFields((current) => current.filter((field) => field !== key))
    const next = { ...drafts }
    for (const param of fieldParams(key)) {
      next[param] = undefined
    }
    setDrafts(next)
    commit(next)
  }

  function clearAll() {
    const cleared = pickFilterParams({})
    setAddedFields([])
    setDrafts((current) => ({ ...current, ...cleared }))
    onApply(cleared)
  }

  return (
    <div className="flex min-w-0 flex-col gap-3" data-testid="agent-session-toolbar">
      <div className="flex min-w-0 flex-wrap items-center justify-end gap-3">
        <span className="text-muted-foreground text-sm tabular-nums" role="status">
          {isPending
            ? 'Loading…'
            : `Showing ${formatCount(shownCount)} of ${formatCount(totalCount)}`}
        </span>
        <div className="flex items-center gap-2">
          <FilterFieldMenu visibleFields={visibleFields} onSelect={addField}>
            <Button type="button" variant="outline" size="sm" className="gap-2">
              <AppIcon
                icon={FilterHorizontalIcon}
                size={14}
                stroke={1.5}
                data-icon="inline-start"
              />
              Filters
              {activeCount > 0 ? (
                <Badge variant="secondary" className="tabular-nums">
                  {activeCount}
                </Badge>
              ) : null}
            </Button>
          </FilterFieldMenu>
          {actions}
        </div>
      </div>

      {visibleFields.length > 0 ? (
        <div
          className="flex min-w-0 flex-wrap items-center gap-2"
          data-testid="agent-session-filter-chips"
        >
          {visibleFields.map((field) => (
            <FilterChip
              key={field.key}
              field={field}
              drafts={drafts}
              autoFocus={focusField === field.key}
              pickerOptions={field.picker ? pickerOptions(field.picker.source) : undefined}
              onPick={pick}
              onDraftChange={(param, value) =>
                setDrafts((current) => ({ ...current, [param]: value }))
              }
              onCommit={() => commit()}
              onRemove={() => removeField(field.key)}
            />
          ))}
          <FilterFieldMenu visibleFields={visibleFields} onSelect={addField}>
            <Button type="button" variant="outline" size="icon-sm" aria-label="Add filter">
              <AppIcon icon={Add01Icon} size={14} stroke={1.5} aria-hidden />
            </Button>
          </FilterFieldMenu>
          <Button type="button" variant="ghost" size="sm" onClick={clearAll} disabled={isPending}>
            Clear
          </Button>
        </div>
      ) : null}

      {isPartialTag(drafts) ? (
        <Alert>
          <AlertTitle>Incomplete tag filter</AlertTitle>
          <AlertDescription>
            Provide both a tag key and tag value to filter sessions by a request tag.
          </AlertDescription>
        </Alert>
      ) : null}
    </div>
  )
}

/** Loads catalog-backed options only once their chip is shown. */
function usePickerOptions(visibleFields: SessionFilterField[]) {
  const shows = (source: PickerSource) =>
    visibleFields.some((field) => field.picker?.source === source)
  const harnessOptions = useLoadedFilterOptions(shows('harnesses'), loadHarnessOptions)
  const modelOptions = useModelFilterOptions(shows('models'))

  return (source: PickerSource): FilterOptionsState => {
    if (source === 'harnesses') return harnessOptions
    if (source === 'models') return modelOptions
    if (source === 'dates') return readyFilterOptions(dayOptions(new Date()))
    return readyFilterOptions(source)
  }
}

/** Harnesses that sent traffic in the last 31 days; older keys can still be typed. */
async function loadHarnessOptions(): Promise<FilterOption[]> {
  const usage = await getObservabilityHarnessUsage({ data: { range: '31d' } })
  return usage.data.leaders
    .map((leader) => ({
      value: leader.agent_harness_key,
      label: leader.agent_harness_label,
      hint: leader.agent_harness_key,
    }))
    .sort((left, right) => left.label.localeCompare(right.label))
}

function FilterFieldMenu({
  visibleFields,
  onSelect,
  children,
}: {
  visibleFields: SessionFilterField[]
  onSelect: (key: SessionFilterFieldKey) => void
  children: ReactNode
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>{children}</DropdownMenuTrigger>
      {/* Keep focus on the newly added chip instead of returning it to the trigger. */}
      <DropdownMenuContent
        align="end"
        className="w-56"
        onCloseAutoFocus={(event) => event.preventDefault()}
      >
        <DropdownMenuLabel>Filter by</DropdownMenuLabel>
        {sessionFilterGroups.map((group, index) => (
          <Fragment key={group.label}>
            {index > 0 ? <DropdownMenuSeparator /> : null}
            <DropdownMenuGroup aria-label={group.label}>
              {group.fields.map((field) => (
                <DropdownMenuItem
                  key={field.key}
                  disabled={visibleFields.some((visible) => visible.key === field.key)}
                  onSelect={() => onSelect(field.key)}
                >
                  <AppIcon icon={field.icon} size={16} stroke={1.5} aria-hidden />
                  {field.label}
                </DropdownMenuItem>
              ))}
            </DropdownMenuGroup>
          </Fragment>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

function FilterChip({
  field,
  drafts,
  autoFocus,
  pickerOptions,
  onPick,
  onDraftChange,
  onCommit,
  onRemove,
}: {
  field: SessionFilterField
  drafts: SessionFilterPatch
  autoFocus: boolean
  pickerOptions?: FilterOptionsState
  onPick: (param: SessionFilterParam, value: string) => void
  onDraftChange: (param: SessionFilterParam, value: string) => void
  onCommit: () => void
  onRemove: () => void
}) {
  const firstInputRef = useRef<HTMLInputElement | null>(null)

  useEffect(() => {
    if (!autoFocus || field.picker) return
    // Defer past the dropdown's own focus restoration on close.
    const handle = setTimeout(() => firstInputRef.current?.focus(), 0)
    return () => clearTimeout(handle)
  }, [autoFocus, field.picker])

  function inputProps(param: SessionFilterParam, label: string, placeholder: string) {
    return {
      'aria-label': label,
      'data-testid': testIdFor(param),
      // Chips span the row on narrow screens, so their inputs share it instead of overflowing.
      className: 'min-w-0 flex-1 sm:w-36 sm:flex-none',
      placeholder,
      value: draftText(drafts[param]),
      onChange: (event: ChangeEvent<HTMLInputElement>) => onDraftChange(param, event.target.value),
      onBlur: onCommit,
      onKeyDown: (event: KeyboardEvent<HTMLInputElement>) => {
        if (event.key === 'Enter') {
          event.preventDefault()
          onCommit()
        }
      },
    }
  }

  function chipValue() {
    if (field.key === 'request_tag') {
      return (
        <>
          <InputGroupInput
            ref={firstInputRef}
            {...inputProps('request_tag_key', 'Request tag key', 'key')}
            className="min-w-0 flex-1 sm:w-24 sm:flex-none"
          />
          <InputGroupText className="border-x px-2">is</InputGroupText>
          <InputGroupInput {...inputProps('request_tag_value', 'Request tag value', 'value')} />
        </>
      )
    }
    const param = field.key
    const picker = field.picker
    return (
      <>
        <InputGroupText className="border-r px-2">is</InputGroupText>
        {picker && pickerOptions ? (
          <FilterValuePicker
            label={field.label}
            value={pickerValue(draftText(drafts[param]), picker.fromParam)}
            placeholder={field.placeholder}
            searchPlaceholder={picker.searchPlaceholder}
            options={pickerOptions}
            autoOpen={autoFocus}
            formatValue={picker.formatValue}
            testId={testIdFor(param)}
            customValue={picker.customValue}
            onSelect={(value) => onPick(param, picker.toParam ? picker.toParam(value) : value)}
          />
        ) : (
          <InputGroupInput
            ref={firstInputRef}
            {...inputProps(param, `${field.label} filter value`, field.placeholder)}
          />
        )}
      </>
    )
  }

  return (
    <InputGroup className="w-full sm:w-auto" data-testid={`agent-session-filter-chip-${field.key}`}>
      <InputGroupAddon className="text-foreground border-r pr-2 whitespace-nowrap">
        <AppIcon icon={field.icon} size={14} stroke={1.5} aria-hidden />
        {field.label}
      </InputGroupAddon>
      {chipValue()}
      <InputGroupAddon align="inline-end" className="border-l pl-1">
        <InputGroupButton
          size="icon-xs"
          aria-label={`Remove ${field.label} filter`}
          onClick={onRemove}
        >
          <AppIcon icon={Cancel01Icon} size={14} stroke={1.5} aria-hidden />
        </InputGroupButton>
      </InputGroupAddon>
    </InputGroup>
  )
}

function testIdFor(param: SessionFilterParam) {
  return `agent-session-filter-${param.replaceAll('_', '-')}`
}

function pickerValue(value: string, fromParam?: (value: string) => string) {
  return value && fromParam ? fromParam(value) : value
}

function isFieldActive(filters: SessionFilterPatch, key: SessionFilterFieldKey) {
  return fieldParams(key).some((param) => normalizedParam(filters[param]) !== undefined)
}

function isPartialTag(values: SessionFilterPatch) {
  return (
    (normalizedParam(values.request_tag_key) === undefined) !==
    (normalizedParam(values.request_tag_value) === undefined)
  )
}

function draftText(value: string | number | null | undefined) {
  return value == null ? '' : String(value)
}

function normalizedParam(value: string | number | null | undefined) {
  return draftText(value).trim() || undefined
}

/** The chip-owned params of `values`, with absent params set to `undefined` so they clear. */
function pickFilterParams(values: SessionFilterPatch): SessionFilterPatch {
  return Object.fromEntries(
    sessionFilterParams.map((param) => [param, normalizedParam(values[param])]),
  )
}

function sameFilters(left: SessionFilterPatch, right: SessionFilterPatch) {
  return sessionFilterParams.every(
    (param) => normalizedParam(left[param]) === normalizedParam(right[param]),
  )
}

/**
 * Moves drafts onto newly applied URL filters. Params whose draft still matches the previous URL
 * value are clean and take the new value; params edited since then keep the in-progress edit.
 */
function rebaseDrafts(
  drafts: SessionFilterPatch,
  previous: SessionFilterPatch,
  next: SessionFilterPatch,
): SessionFilterPatch {
  const rebased: SessionFilterPatch = { ...next }
  for (const param of sessionFilterParams) {
    const isDirty = normalizedParam(drafts[param]) !== normalizedParam(previous[param])
    if (isDirty) {
      rebased[param] = drafts[param]
    }
  }
  return rebased
}
