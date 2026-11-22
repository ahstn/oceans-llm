import type { IconSvgElement } from '@hugeicons/react'
import {
  useEffect,
  useRef,
  useState,
  type ChangeEvent,
  type KeyboardEvent,
  type ReactNode,
} from 'react'
import {
  Add01Icon,
  AiChipIcon,
  Cancel01Icon,
  CloudServerIcon,
  CubeIcon,
  FilterHorizontalIcon,
  HashIcon,
  Leaf01Icon,
  Package01Icon,
  Search01Icon,
  StatusIcon,
  Tag01Icon,
} from '@hugeicons/core-free-icons'

import {
  anyTypedValue,
  useModelFilterOptions,
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
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import {
  InputGroup,
  InputGroupAddon,
  InputGroupButton,
  InputGroupInput,
  InputGroupText,
} from '@/components/ui/input-group'
import type { RequestLogFiltersInput } from '@/types/api'

import { customStatusCode, readyStatusCodeOptions } from './-request-log-filter-options'

export type RequestLogFilterValues = Pick<
  RequestLogFiltersInput,
  | 'q'
  | 'request_id'
  | 'model_key'
  | 'provider_key'
  | 'status_code'
  | 'service'
  | 'component'
  | 'env'
  | 'tag_key'
  | 'tag_value'
>

/**
 * Chip drafts and URL patches. Values are kept as typed; the route turns the status code back
 * into a number when it normalizes the search.
 */
export type RequestLogFilterPatch = {
  [Param in keyof RequestLogFilterValues]?: string | number | null
}

type FilterParam = Exclude<keyof RequestLogFilterValues, 'q'>
type FilterFieldKey = Exclude<FilterParam, 'tag_key' | 'tag_value'> | 'tag'
type PickerFieldKey = Extract<FilterFieldKey, 'model_key' | 'status_code'>

interface FilterFieldDefinition {
  key: FilterFieldKey
  label: string
  icon: IconSvgElement
  placeholder: string
  /** Fields with a known set of values pick from a list instead of free text. */
  picker?: { searchPlaceholder: string; customValue: (search: string) => string | null }
}

const filterFields: FilterFieldDefinition[] = [
  {
    key: 'model_key',
    label: 'Model',
    icon: AiChipIcon,
    placeholder: 'Select a model',
    // Keys no longer in the catalog still match older logs, so any typed key is allowed.
    picker: { searchPlaceholder: 'Search models…', customValue: anyTypedValue },
  },
  {
    key: 'provider_key',
    label: 'Provider',
    icon: CloudServerIcon,
    placeholder: 'openai',
  },
  {
    key: 'status_code',
    label: 'Status',
    icon: StatusIcon,
    placeholder: 'Select a status',
    picker: { searchPlaceholder: 'Search or type a code…', customValue: customStatusCode },
  },
  {
    key: 'service',
    label: 'Service',
    icon: Package01Icon,
    placeholder: 'checkout',
  },
  { key: 'component', label: 'Component', icon: CubeIcon, placeholder: 'api' },
  { key: 'env', label: 'Environment', icon: Leaf01Icon, placeholder: 'prod' },
  { key: 'tag', label: 'Tag', icon: Tag01Icon, placeholder: 'value' },
  {
    key: 'request_id',
    label: 'Request ID',
    icon: HashIcon,
    placeholder: 'req_…',
  },
]

const searchDebounceMs = 250

interface RequestLogToolbarProps {
  filters: RequestLogFilterValues
  shownCount: number
  totalCount: number
  isPending: boolean
  /** Merges `patch` into the current URL search; keys set to `undefined` are removed. */
  onApply: (patch: RequestLogFilterPatch, options?: { replace?: boolean }) => void
}

/**
 * ReUI-style request log toolbar: a debounced model/user search plus removable
 * `[field | is | value | ×]` filter chips that commit to the URL on Enter or blur.
 */
// Search, chip drafts, and the add-filter menus share one URL-backed filter state.
// oxlint-disable-next-line eslint/max-lines-per-function
export function RequestLogToolbar({
  filters,
  shownCount,
  totalCount,
  isPending,
  onApply,
}: RequestLogToolbarProps) {
  const [drafts, setDrafts] = useState<RequestLogFilterPatch>(filters)
  const [addedFields, setAddedFields] = useState<FilterFieldKey[]>([])
  const [focusField, setFocusField] = useState<FilterFieldKey | null>(null)

  const [syncedFilters, setSyncedFilters] = useState(filters)
  // The filters most recently sent to `onApply`. While its navigation is pending, `filters` still
  // holds the earlier URL, so changes are compared against what was sent instead. Once nothing is
  // pending the URL is the truth again, so a navigation that was cancelled can be retried.
  const [appliedFilters, setAppliedFilters] = useState<RequestLogFilterPatch>(filters)
  const submittedFilters = isPending ? appliedFilters : filters

  // Adopt URL-backed filter changes (navigation, Back/Forward, Clear) without discarding chip
  // edits that were typed while a previous navigation was still loading.
  if (syncedFilters !== filters) {
    setSyncedFilters(filters)
    setAppliedFilters(filters)
    setDrafts((current) => rebaseDrafts(current, syncedFilters, filters))
  }

  const visibleFields = filterFields.filter(
    (field) => isFieldActive(filters, field.key) || addedFields.includes(field.key),
  )
  const activeCount = filterFields.filter((field) => isFieldActive(filters, field.key)).length
  const hasPartialTagFilter = isPartialTag(drafts)
  // The catalog loads on first use (adding the Model chip or opening its picker), so a page opened
  // with `model_key` already in the URL does not fetch it.
  const [modelPickerUsed, setModelPickerUsed] = useState(false)
  const modelOptions = useModelFilterOptions(modelPickerUsed)
  function loadModelOptions() {
    setModelPickerUsed(true)
    modelOptions.load?.()
  }
  const pickerOptions: Record<PickerFieldKey, FilterOptionsState> = {
    model_key: { ...modelOptions, load: loadModelOptions },
    status_code: readyStatusCodeOptions,
  }

  function addField(key: FilterFieldKey) {
    setAddedFields((current) => (current.includes(key) ? current : [...current, key]))
    setFocusField(key)
    // The new chip's picker opens itself, so this is the catalog's first use.
    if (key === 'model_key') loadModelOptions()
  }

  function submit(patch: RequestLogFilterPatch) {
    setAppliedFilters({ ...submittedFilters, ...patch })
    onApply(patch)
  }

  function commit(next: RequestLogFilterPatch = drafts) {
    if (isPartialTag(next) || sameFilters(next, submittedFilters)) {
      return
    }
    submit(pickFilterParams(next))
  }

  // Picked values have no draft stage, so they apply as soon as they are chosen. Only the picked
  // param navigates: other chips' drafts, such as a half-typed tag pair, stay local until they
  // commit on their own.
  function pick(param: FilterParam, value: string) {
    setDrafts((current) => ({ ...current, [param]: value }))
    if (normalizedParam(value) !== normalizedParam(submittedFilters[param])) {
      submit({ [param]: normalizedParam(value) })
    }
  }

  function removeField(key: FilterFieldKey) {
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
    submit(cleared)
  }

  return (
    <div className="flex min-w-0 flex-col gap-3" data-testid="request-log-toolbar">
      <div className="flex min-w-0 flex-wrap items-center justify-between gap-3">
        <RequestLogSearch
          value={filters.q ?? ''}
          onSearch={(q) => onApply({ q }, { replace: true })}
        />
        <div className="flex items-center gap-3">
          <span className="text-muted-foreground text-sm tabular-nums">
            {isPending ? 'Loading…' : `Showing ${shownCount} of ${totalCount}`}
          </span>
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
        </div>
      </div>

      {visibleFields.length > 0 ? (
        <div
          className="flex min-w-0 flex-wrap items-center gap-2"
          data-testid="request-log-filter-chips"
        >
          {visibleFields.map((field) => (
            <FilterChip
              key={field.key}
              field={field}
              drafts={drafts}
              autoFocus={focusField === field.key}
              pickerOptions={field.picker ? pickerOptions[field.key as PickerFieldKey] : undefined}
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

      {hasPartialTagFilter ? (
        <Alert>
          <AlertTitle>Incomplete tag filter</AlertTitle>
          <AlertDescription>
            Provide both a tag key and tag value to filter bespoke request tags.
          </AlertDescription>
        </Alert>
      ) : null}
    </div>
  )
}

function RequestLogSearch({ value, onSearch }: { value: string; onSearch: (q: string) => void }) {
  const [query, setQuery] = useState(value)
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null)
  // The debounce timer outlives the render that scheduled it, so read the latest props from refs.
  const latest = useRef({ value, onSearch })

  useEffect(() => {
    latest.current = { value, onSearch }
  })

  useEffect(() => {
    // Only adopt URL changes (Back/Forward, Clear) when no keystrokes are waiting to be sent.
    if (timer.current === null) {
      setQuery(value)
    }
  }, [value])

  useEffect(() => () => clearTimeout(timer.current ?? undefined), [])

  function updateQuery(next: string) {
    setQuery(next)
    clearTimeout(timer.current ?? undefined)
    timer.current = setTimeout(() => {
      timer.current = null
      if (next.trim() !== latest.current.value) {
        latest.current.onSearch(next)
      }
    }, searchDebounceMs)
  }

  return (
    <InputGroup className="w-full sm:max-w-xs">
      <InputGroupInput
        type="search"
        aria-label="Search request logs by model or user"
        placeholder="Search by model or user…"
        value={query}
        onChange={(event) => updateQuery(event.target.value)}
      />
      <InputGroupAddon>
        <AppIcon icon={Search01Icon} aria-hidden />
      </InputGroupAddon>
    </InputGroup>
  )
}

function FilterFieldMenu({
  visibleFields,
  onSelect,
  children,
}: {
  visibleFields: FilterFieldDefinition[]
  onSelect: (key: FilterFieldKey) => void
  children: ReactNode
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>{children}</DropdownMenuTrigger>
      {/* Keep focus on the newly added chip input instead of returning it to the trigger. */}
      <DropdownMenuContent
        align="end"
        className="w-48"
        onCloseAutoFocus={(event) => event.preventDefault()}
      >
        <DropdownMenuGroup>
          <DropdownMenuLabel>Filter by</DropdownMenuLabel>
          {filterFields.map((field) => (
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
  field: FilterFieldDefinition
  drafts: RequestLogFilterPatch
  autoFocus: boolean
  pickerOptions?: FilterOptionsState
  onPick: (param: FilterParam, value: string) => void
  onDraftChange: (param: FilterParam, value: string) => void
  onCommit: () => void
  onRemove: () => void
}) {
  const firstInputRef = useRef<HTMLInputElement | null>(null)

  useEffect(() => {
    if (!autoFocus) return
    // Defer past the dropdown's own focus restoration on close.
    const handle = setTimeout(() => firstInputRef.current?.focus(), 0)
    return () => clearTimeout(handle)
  }, [autoFocus])

  function inputProps(param: FilterParam, label: string, placeholder: string) {
    return {
      'aria-label': label,
      'data-testid': `request-log-filter-${param.replace('_', '-')}`,
      className: 'w-36 flex-none',
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

  return (
    <InputGroup className="w-auto" data-testid={`request-log-filter-chip-${field.key}`}>
      <InputGroupAddon className="text-foreground border-r pr-2">
        <AppIcon icon={field.icon} size={14} stroke={1.5} aria-hidden />
        {field.label}
      </InputGroupAddon>
      {field.key === 'tag' ? (
        <>
          <InputGroupInput
            ref={firstInputRef}
            {...inputProps('tag_key', 'Tag key', 'key')}
            className="w-24 flex-none"
          />
          <InputGroupText className="border-x px-2">is</InputGroupText>
          <InputGroupInput {...inputProps('tag_value', 'Tag value', field.placeholder)} />
        </>
      ) : field.picker && pickerOptions ? (
        <>
          <InputGroupText className="border-r px-2">is</InputGroupText>
          <FilterValuePicker
            label={field.label}
            value={draftText(drafts[field.key])}
            placeholder={field.placeholder}
            searchPlaceholder={field.picker.searchPlaceholder}
            options={pickerOptions}
            autoOpen={autoFocus}
            showSelectedHint={field.key === 'status_code'}
            testId={`request-log-filter-${field.key.replace('_', '-')}`}
            customValue={field.picker.customValue}
            onSelect={(value) => onPick(field.key as FilterParam, value)}
          />
        </>
      ) : (
        <>
          <InputGroupText className="border-r px-2">is</InputGroupText>
          <InputGroupInput
            ref={firstInputRef}
            {...inputProps(field.key, `${field.label} filter value`, field.placeholder)}
          />
        </>
      )}
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

function fieldParams(key: FilterFieldKey): FilterParam[] {
  return key === 'tag' ? ['tag_key', 'tag_value'] : [key]
}

function isFieldActive(filters: RequestLogFilterValues, key: FilterFieldKey) {
  return fieldParams(key).some((param) => normalizedParam(filters[param]) !== undefined)
}

function isPartialTag(values: RequestLogFilterPatch) {
  return (
    (normalizedParam(values.tag_key) === undefined) !==
    (normalizedParam(values.tag_value) === undefined)
  )
}

const filterParams: FilterParam[] = filterFields.flatMap((field) => fieldParams(field.key))

function draftText(value: string | number | null | undefined) {
  return value == null ? '' : String(value)
}

function normalizedParam(value: string | number | null | undefined) {
  return draftText(value).trim() || undefined
}

/** The chip-owned params of `values`, with absent params set to `undefined` so they clear. */
function pickFilterParams(values: RequestLogFilterPatch): RequestLogFilterPatch {
  return Object.fromEntries(filterParams.map((param) => [param, normalizedParam(values[param])]))
}

function sameFilters(left: RequestLogFilterPatch, right: RequestLogFilterPatch) {
  return filterParams.every(
    (param) => normalizedParam(left[param]) === normalizedParam(right[param]),
  )
}

/**
 * Moves drafts onto newly applied URL filters. Params whose draft still matches the previous URL
 * value are clean and take the new value; params edited since then keep the in-progress edit.
 */
function rebaseDrafts(
  drafts: RequestLogFilterPatch,
  previous: RequestLogFilterValues,
  next: RequestLogFilterValues,
): RequestLogFilterPatch {
  const rebased: RequestLogFilterPatch = { ...next }
  for (const param of filterParams) {
    const isDirty = normalizedParam(drafts[param]) !== normalizedParam(previous[param])
    if (isDirty) {
      rebased[param] = drafts[param]
    }
  }
  return rebased
}
